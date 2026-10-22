//! What `martin test` needs from a backend, and the tables it reads through one.

use martin_core::tiles::BoxedSource;
use martin_tilegen::TileGenError;
use martin_tilegen::plan::{Branch, ColumnValues, Condition, GeometryType, Plan, Shape, TableDef};
use martin_tilegen::source::FeatureSource;

use crate::generate::GenerateResult;

/// The key of a row: the value of its table's key column.
pub type RowKey = u64;

/// A table source with layers, as `martin test` reads it.
#[derive(Clone, Debug)]
pub struct TestTable {
    pub source: String,
    /// The table, lowered for zooms 0 to the max zoom of the spec.
    pub def: TableDef,
    /// The only values the source allows some columns.
    pub known: Vec<ColumnValues>,
    /// The column holding the key of each row, which a case selects its row by.
    pub key: Option<String>,
    /// The geometry type of every row, when the source declares a single one.
    pub geometry: Option<GeometryType>,
}

impl TestTable {
    /// A plan of this table alone, as table 0.
    ///
    /// # Errors
    ///
    /// If the table cannot be planned.
    pub fn plan(&self) -> GenerateResult<Plan> {
        Ok(Plan::new(vec![self.def.clone()]).map_err(TileGenError::from)?)
    }

    /// Every path through the layers, without shapes for a table of points.
    ///
    /// # Errors
    ///
    /// If the table cannot be planned.
    pub fn paths(&self) -> GenerateResult<Vec<Branch>> {
        let points = self.geometry == Some(GeometryType::Point);
        Ok(self
            .plan()?
            .branches(0, &self.known)?
            .into_iter()
            .filter(|branch| !(points && branch.shape.is_some()))
            .collect())
    }
}

/// Which rows a scan reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
    /// Those bulk mode reads: only rows some layer takes, if the backend can tell.
    Bulk,
    /// Every row of the source, also those no layer takes.
    All,
}

/// What a backend's search for a row on a path found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Find {
    /// The backend cannot search, so the runner sweeps a scan of the table instead.
    Unsupported,
    /// The rows the backend selected, e.g. as SQL, and the key of one of them if any.
    Searched { rows: String, key: Option<RowKey> },
    /// The rows the backend tried to select, and why it could not.
    Failed { rows: String, error: String },
}

/// What `martin test` needs from a backend.
#[expect(
    async_fn_in_trait,
    reason = "the runner awaits one backend in place, never on another thread"
)]
pub trait TestBackend {
    /// The rows of a scan, as bulk mode reads them.
    type Scan: FeatureSource + Send + 'static;

    /// Sources with layers, lowered for zooms 0 to `max_zoom`.
    ///
    /// # Errors
    ///
    /// If a source cannot be lowered.
    fn tables(&self, max_zoom: u8) -> GenerateResult<Vec<TestTable>>;

    /// The rows of `table`, only row `key` when given.
    async fn scan(&self, table: &TestTable, key: Option<RowKey>, rows: Rows) -> GenerateResult<Self::Scan>;

    /// A row of `table` on `rows` with geometry of `shape`, selected by the backend itself.
    async fn find(&self, _table: &TestTable, _rows: &Condition, _shape: Option<Shape>) -> Find {
        Find::Unsupported
    }

    /// Row `key` of `table` as the tile server serves it, up to `max_zoom` or else the source's own, or `None` when
    /// the backend has no server.
    async fn serve(
        &self,
        _table: &TestTable,
        _key: RowKey,
        _max_zoom: Option<u8>,
    ) -> GenerateResult<Option<BoxedSource>> {
        Ok(None)
    }
}
