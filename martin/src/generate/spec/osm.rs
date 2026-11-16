//! The OpenStreetMap backend of `martin test`: the `osm` sources of a configuration, each case one object of a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use martin_tilegen::TileGenError;
use martin_tilegen::osm::{NodeStore, OSM_TYPE, Objects, OsmSource};
use martin_tilegen::plan::{ColumnFacts, Condition, Literal, Shape};
use martin_tilegen::source::{FeatureSource as _, SourceError};

use crate::config::file::osm::OsmSourceConfig;
use crate::config::file::{Config, read_config};
use crate::config::primitives::env::OsEnv;
use crate::generate::layers::LowerOptions;
use crate::generate::osm::{lower, partition_bytes, table};
use crate::generate::spec::backend::{Find, RowKey, Rows, TestBackend, TestTable};
use crate::generate::{GenerateError, GenerateResult};

/// The input key holding a case's feature id: the OSM id times 10, plus 1 for a node, 2 for a way, 3 for a relation.
pub const FEATURE_ID: &str = "id";

/// The input key holding a case's geometry as WKT.
const GEOMETRY: &str = "geometry";

/// The `osm` sources of a configuration.
pub struct Osm {
    sources: BTreeMap<String, Source>,
    /// Where node stores too large for memory are kept.
    temp_dir: PathBuf,
}

/// One `osm` source, and the node store its reads share once one opened it.
struct Source {
    config: OsmSourceConfig,
    nodes: OnceLock<NodeStore>,
}

impl Osm {
    /// The `osm` sources of the configuration at `config_path`, or `None` when it has none.
    ///
    /// # Errors
    ///
    /// If the configuration cannot be read, also has database sources, or a source reads the tag [`FEATURE_ID`] or
    /// `geometry`.
    pub fn load(config_path: &Path) -> GenerateResult<Option<Self>> {
        let config: Config = read_config(config_path, &OsEnv)?;
        if config.osm.sources.is_empty() {
            return Ok(None);
        }
        if !config.postgres.is_empty() {
            return Err(GenerateError::MixedTestSources);
        }
        for (id, source) in &config.osm.sources {
            if let Some(tag) = source
                .tags
                .iter()
                .find(|tag| [FEATURE_ID, GEOMETRY].contains(&tag.as_str()))
            {
                return Err(GenerateError::OsmReservedTag {
                    source_id: id.clone(),
                    tag: tag.clone(),
                });
            }
        }
        let sources = config
            .osm
            .sources
            .into_iter()
            .map(|(id, config)| {
                let nodes = OnceLock::new();
                (id, Source { config, nodes })
            })
            .collect();
        Ok(Some(Self {
            sources,
            temp_dir: std::env::temp_dir(),
        }))
    }

    /// A read of `table`'s file for `objects`, sharing the node locations of every other read of it.
    async fn open(&self, table: &TestTable, objects: Objects) -> GenerateResult<OsmSource> {
        let Some(source) = self.sources.get(&table.source) else {
            let known = self.sources.keys().cloned().collect::<Vec<_>>();
            return Err(GenerateError::UnknownSource(table.source.clone(), known.join(", ")));
        };
        let mut osm = self::table(&source.config, &table.def, &self.temp_dir, objects)?;
        osm.node_store = source.nodes.get_or_init(|| osm.node_store.clone().shared()).clone();
        let tables = vec![(table.def.clone(), osm)];
        let bytes = partition_bytes(&tables, 1);
        Ok(tokio::task::spawn_blocking(move || OsmSource::new(tables, bytes)).await??)
    }

    /// The feature id of the first object of `table`'s file, in file order, for which `rows` holds and whose geometry
    /// has `shape`.
    async fn search(
        &self,
        table: &TestTable,
        rows: &Condition,
        shape: Option<Shape>,
    ) -> GenerateResult<Option<RowKey>> {
        let scan = self.open(table, Objects::Matching(rows.clone())).await?;
        tokio::task::spawn_blocking(move || {
            let mut found = None;
            for partition in 0..scan.partitions() {
                let read = scan.read(partition, scan.plan().table_keys(), &mut |batch| {
                    for feature in &batch.features {
                        let fits = match shape {
                            Some(shape) => shape
                                .fits(batch.crs, &feature.geometry)
                                .map_err(|err| SourceError::Other(Box::new(err)))?,
                            None => true,
                        };
                        if fits && feature.id.is_some() {
                            found = feature.id;
                            return Err(SourceError::Stopped);
                        }
                    }
                    Ok(())
                });
                match read {
                    Ok(()) => {}
                    Err(SourceError::Stopped) => return Ok(found),
                    Err(err) => return Err(TileGenError::from(err).into()),
                }
            }
            scan.finish().map_err(TileGenError::from)?;
            Ok(found)
        })
        .await?
    }
}

impl TestBackend for Osm {
    type Scan = OsmSource;

    fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
        let options = LowerOptions {
            zooms: 0..=max_zoom,
            bounds: None,
            encoder: None,
        };
        let mut out = Vec::with_capacity(self.sources.len());
        for (id, source) in &self.sources {
            let Some(def) = lower(id, &source.config, &options)? else {
                continue;
            };
            let mut table = TestTable {
                source: id.clone(),
                def,
                facts: Vec::new(),
                key: Some(FEATURE_ID.to_owned()),
                geometry: None,
            };
            table.learn(ColumnFacts {
                column: OSM_TYPE.to_owned(),
                not_null: true,
                values: Some(
                    ["node", "way", "relation"]
                        .into_iter()
                        .map(|kind| Literal::String(kind.to_owned()))
                        .collect(),
                ),
            });
            out.push(table);
        }
        Ok(out)
    }

    /// A read of the source's whole file: only object `key` when given, else every tagged object for [`Rows::All`].
    async fn scan(&self, table: &TestTable, key: Option<RowKey>, rows: Rows) -> GenerateResult<OsmSource> {
        let objects = match (key, rows) {
            (Some(key), _) => Objects::One(key),
            (None, Rows::All) => Objects::Every,
            (None, Rows::Bulk) => Objects::Taken,
        };
        self.open(table, objects).await
    }

    /// The first object of the file on the path, in file order.
    async fn find(&self, table: &TestTable, rows: &Condition, shape: Option<Shape>) -> Find {
        let selected = rows.to_string();
        match self.search(table, rows, shape).await {
            Ok(key) => Find::Searched { rows: selected, key },
            Err(error) => Find::Failed {
                rows: selected,
                error: error.to_string(),
            },
        }
    }
}
