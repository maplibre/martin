//! The shapefile backend of `martin test`: the `shapefile` sources of a configuration, each case one record of a file.

use std::collections::BTreeMap;
use std::path::Path;

use martin_tilegen::shapefile::{Records, Shapefile, ShapefileSource, ShapefileTable};

use crate::config::file::shapefile::ShapefileSourceConfig;
use crate::config::file::{Config, read_config};
use crate::config::primitives::env::OsEnv;
use crate::generate::layers::LowerOptions;
use crate::generate::shapefile::{lower, open};
use crate::generate::spec::backend::{RowKey, Rows, TestBackend, TestTable};
use crate::generate::{GenerateError, GenerateResult};

/// The input key holding a case's record number, counting from 1, which is also its feature id.
pub const RECORD: &str = "record";

/// The `shapefile` sources of a configuration, each file opened once.
pub struct Shapefiles {
    sources: BTreeMap<String, (ShapefileSourceConfig, Shapefile)>,
}

impl Shapefiles {
    /// The `shapefile` sources of the configuration at `config_path`, or `None` when it has none. A zip file is
    /// unpacked into the temporary directory until the backend is dropped.
    ///
    /// # Errors
    ///
    /// If the configuration or a file cannot be read, it also has database sources, or a file has a field
    /// [`RECORD`].
    pub fn load(config_path: &Path) -> GenerateResult<Option<Self>> {
        let config: Config = read_config(config_path, &OsEnv)?;
        if config.shapefile.sources.is_empty() {
            return Ok(None);
        }
        if !config.postgres.is_empty() {
            return Err(GenerateError::MixedTestSources);
        }
        let mut sources = BTreeMap::new();
        for (id, source) in config.shapefile.sources {
            let file = open(&source, &std::env::temp_dir())?;
            if file.has_field(RECORD) {
                return Err(GenerateError::ShapefileRecordField {
                    source_id: id,
                    field: RECORD.to_owned(),
                });
            }
            sources.insert(id, (source, file));
        }
        Ok(Some(Self { sources }))
    }

    /// Whether source `id` is one of these.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.sources.contains_key(id)
    }
}

impl TestBackend for Shapefiles {
    type Scan = ShapefileSource;

    fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
        let options = LowerOptions {
            zooms: 0..=max_zoom,
            bounds: None,
            encoder: None,
            order_by: None,
        };
        let mut out = Vec::with_capacity(self.sources.len());
        for (id, (source, file)) in &self.sources {
            let Some(def) = lower(id, source, file, &options)? else {
                continue;
            };
            out.push(TestTable {
                source: id.clone(),
                def,
                facts: Vec::new(),
                key: Some(RECORD.to_owned()),
                geometry: file.geometry(),
            });
        }
        Ok(out)
    }

    /// Every record of the source's file, or only record `key`; nothing is pushed down, so [`Rows`] does not matter.
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "the file is open already, other backends wait for theirs"
    )]
    async fn scan(&self, table: &TestTable, key: Option<RowKey>, _rows: Rows) -> GenerateResult<ShapefileSource> {
        let Some((_, file)) = self.sources.get(&table.source) else {
            let known = self.sources.keys().cloned().collect::<Vec<_>>();
            return Err(GenerateError::UnknownSource(table.source.clone(), known.join(", ")));
        };
        let read = ShapefileTable {
            file: file.clone(),
            bounds: None,
            records: key.map_or(Records::All, Records::One),
        };
        Ok(ShapefileSource::new(vec![(table.def.clone(), read)])?)
    }
}
