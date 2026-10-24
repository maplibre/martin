//! Phase 1, read and render: every feature into sorted records, by one [`RenderWorker`] per thread.

pub mod project;
mod render;
mod simplify;

use geo_types::Coord;
use martin_tile_utils::TileRect;

pub use crate::phase1::render::{Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError};
use crate::phase1::render::{Renderer, SliceErrors};
use crate::plan::{Evaluator, ExprTallies, Plan};
use crate::sort::{Seq, SortBuffer, Sorter};
use crate::source::{Crs, FeatureBatch, Geometry, WkbError};
use crate::{LayerSliceErrors, Summary, TileGenError, TileGenResult, TileOrder};

/// The stage types of phase 1, for this crate's benches and stage-level tests only.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    pub use crate::phase1::render::{Renderer, SliceError, SliceErrors};
}

/// Renders feature batches into its own buffer of the sorter, for one thread.
pub(crate) struct RenderWorker<'a> {
    renderer: Renderer,
    evaluator: Evaluator<'a>,
    /// The plan's layers narrowed to a window, by layer index; `None` renders the plan's own.
    windowed: Option<Vec<RenderLayer>>,
    order: TileOrder,
    buffer: SortBuffer<'a>,
    features: u64,
    skipped: u64,
}

impl<'a> RenderWorker<'a> {
    /// A worker that renders `plan`'s layers into `sorter`, keyed for tiles in `order`, only those
    /// of `window` when given.
    pub(crate) fn new(plan: &'a Plan, sorter: &'a Sorter, order: TileOrder, window: Option<TileRect>) -> Self {
        let windowed = window.map(|window| {
            plan.layers
                .iter()
                .map(|layer| layer.render.clone().with_window(window))
                .collect()
        });
        Self {
            renderer: Renderer::default(),
            evaluator: plan.evaluator(),
            windowed,
            order,
            buffer: sorter.buffer(),
            features: 0,
            skipped: 0,
        }
    }

    /// Projects every feature of `batch` in place and sorts the records it renders.
    #[hotpath::measure]
    pub(crate) fn render_batch(&mut self, batch: &mut FeatureBatch) -> TileGenResult<()> {
        let Self {
            renderer,
            evaluator,
            windowed,
            order,
            buffer,
            features,
            skipped,
        } = self;
        for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
            let drawable = project_geometry(batch.crs, &mut feature.geometry).map_err(|error| match feature.id {
                Some(id) => TileGenError::FeatureWkb {
                    table: batch.table,
                    id,
                    error,
                },
                None => TileGenError::RowWkb {
                    table: batch.table,
                    partition: batch.partition,
                    row,
                    error,
                },
            })?;
            let Some(geom) = feature.geometry.feature_geom().filter(|_| drawable) else {
                *skipped += 1;
                continue;
            };
            *features += 1;
            let seq = Seq::new(batch.partition, row)?;
            evaluator.evaluate(batch.table, feature, geom, |evaluated| {
                let layer = evaluated.layer();
                let layer = windowed
                    .as_ref()
                    .map_or(layer, |layers| &layers[usize::from(layer.index)]);
                renderer.render(*order, layer, seq, evaluated.feature(), buffer)
            })?;
        }
        Ok(())
    }

    /// Spills the rest of the buffer, and tells what this worker saw and could not render.
    pub(crate) fn finish(self) -> TileGenResult<RenderTotals> {
        self.buffer.finish()?;
        Ok(RenderTotals {
            features: self.features,
            skipped: self.skipped,
            slice_errors: self.renderer.slice_errors,
            exprs: self.evaluator.finish(),
        })
    }
}

/// Counts and failures of one or more [`RenderWorker`]s.
#[derive(Default)]
pub(crate) struct RenderTotals {
    features: u64,
    skipped: u64,
    /// By layer index.
    slice_errors: Vec<SliceErrors>,
    exprs: ExprTallies,
}

impl RenderTotals {
    /// Adds another worker's totals to these.
    pub(crate) fn add(&mut self, other: Self) {
        self.features += other.features;
        self.skipped += other.skipped;
        if self.slice_errors.len() < other.slice_errors.len() {
            self.slice_errors
                .resize_with(other.slice_errors.len(), SliceErrors::default);
        }
        for (total, errors) in self.slice_errors.iter_mut().zip(other.slice_errors) {
            total.merge(errors);
        }
        self.exprs.merge(other.exprs);
    }

    /// The summary of a render of `plan` that produced `tiles`.
    pub(crate) fn into_summary(self, plan: &Plan, tiles: u64) -> Summary {
        Summary {
            features: self.features,
            skipped: self.skipped,
            tiles,
            slice_errors: layer_slice_errors(plan, &self.slice_errors),
            expr_errors: plan.expr_errors(&self.exprs),
        }
    }
}

/// The first slice error of every layer that had any, by layer name.
fn layer_slice_errors(plan: &Plan, totals: &[SliceErrors]) -> Vec<LayerSliceErrors> {
    plan.layers
        .iter()
        .filter_map(|layer| {
            let index = usize::from(layer.render.index);
            let errors = totals.get(index)?;
            let first = errors.first.as_ref()?;
            Some(LayerSliceErrors {
                layer: layer.info.name.clone(),
                errors: errors.count,
                zoom: first.zoom,
                id: first.id,
                partition: first.seq.partition(),
                row: first.seq.row(),
                error: first.error.to_string(),
            })
        })
        .collect()
}

/// Decodes and projects the geometry in place; `Ok(false)` if it is empty and stays WKB.
pub(crate) fn project_geometry(crs: Crs, geometry: &mut Geometry) -> Result<bool, WkbError> {
    if !geometry.decode_wkb()? {
        return Ok(false);
    }
    match geometry {
        Geometry::Point(point) => project_coords(crs, std::slice::from_mut(point)),
        Geometry::Line(line) => project_coords(crs, &mut line.0),
        Geometry::Polygon(polygon) => {
            polygon.exterior_mut(|ring| project_coords(crs, &mut ring.0));
            polygon.interiors_mut(|rings| {
                for ring in rings {
                    project_coords(crs, &mut ring.0);
                }
            });
        }
        Geometry::Points(points) => project_coords(crs, points),
        Geometry::Lines(lines) => {
            for line in lines.iter_mut() {
                project_coords(crs, &mut line.0);
            }
        }
        Geometry::Polygons(polygons) => {
            for polygon in polygons.iter_mut() {
                polygon.exterior_mut(|ring| project_coords(crs, &mut ring.0));
                polygon.interiors_mut(|rings| {
                    for ring in rings {
                        project_coords(crs, &mut ring.0);
                    }
                });
            }
        }
        Geometry::Wkb(_) => {}
    }
    Ok(true)
}

/// Projects coordinates from `crs` into the renderer's unit square.
fn project_coords(crs: Crs, coords: &mut [Coord<f64>]) {
    match crs {
        Crs::Wgs84 => coords.iter_mut().for_each(|c| *c = project::from_lonlat(*c)),
        Crs::WebMercator => coords.iter_mut().for_each(|c| *c = project::from_mercator(*c)),
    }
}
