//! The file backend of `martin test`: the `shapefile` and `osm` sources of one configuration together.

use std::path::Path;

use martin_tilegen::TileGenError;
use martin_tilegen::plan::{Condition, Shape};
use martin_tilegen::source::{FeatureSource, SourceChain};

use crate::generate::spec::backend::{Find, RowKey, Rows, TestBackend, TestTable};
use crate::generate::spec::osm::Osm;
use crate::generate::spec::shapefile::Shapefiles;
use crate::generate::{GenerateError, GenerateResult};

/// The file sources of a configuration: its shapefile sources, then its OSM sources.
pub struct Files {
    shapefiles: Option<Shapefiles>,
    osm: Option<Osm>,
}

impl Files {
    /// The file sources of the configuration at `config_path`, or `None` when it has none.
    ///
    /// # Errors
    ///
    /// If the configuration or a file cannot be read, or it also has database sources.
    pub fn load(config_path: &Path) -> GenerateResult<Option<Self>> {
        let shapefiles = Shapefiles::load(config_path)?;
        let osm = Osm::load(config_path)?;
        if shapefiles.is_none() && osm.is_none() {
            return Ok(None);
        }
        Ok(Some(Self { shapefiles, osm }))
    }

    /// The shapefile sources, if `source` is one of them.
    fn shapefiles_with(&self, source: &str) -> Option<&Shapefiles> {
        self.shapefiles.as_ref().filter(|shapefiles| shapefiles.has(source))
    }

    /// The OSM sources, as the owner of every source that is not a shapefile source.
    fn osm(&self, source: &str) -> GenerateResult<&Osm> {
        self.osm
            .as_ref()
            .ok_or_else(|| GenerateError::UnknownSource(source.to_owned(), String::new()))
    }
}

/// `source` as a chain of one source reading `table`.
fn chain(source: impl FeatureSource + Send + 'static, table: &TestTable) -> GenerateResult<SourceChain> {
    Ok(SourceChain::new(vec![(Box::new(source), vec![table.def.clone()])]).map_err(TileGenError::from)?)
}

impl TestBackend for Files {
    type Scan = SourceChain;

    fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>> {
        let mut tables = Vec::new();
        if let Some(shapefiles) = &self.shapefiles {
            tables.extend(shapefiles.tables(max_zoom)?);
        }
        if let Some(osm) = &self.osm {
            tables.extend(osm.tables(max_zoom)?);
        }
        Ok(tables)
    }

    async fn scan(&self, table: &TestTable, key: Option<RowKey>, rows: Rows) -> GenerateResult<SourceChain> {
        match self.shapefiles_with(&table.source) {
            Some(shapefiles) => chain(shapefiles.scan(table, key, rows).await?, table),
            None => chain(self.osm(&table.source)?.scan(table, key, rows).await?, table),
        }
    }

    async fn find(&self, table: &TestTable, rows: &Condition, shape: Option<Shape>) -> Find {
        if let Some(shapefiles) = self.shapefiles_with(&table.source) {
            return shapefiles.find(table, rows, shape).await;
        }
        match self.osm(&table.source) {
            Ok(osm) => osm.find(table, rows, shape).await,
            Err(_) => Find::Unsupported,
        }
    }
}
