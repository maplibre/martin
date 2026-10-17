//! The stages every tile render runs through: features into sorted records, sorted records into tiles.

use geo_types::Coord;
use martin_tile_utils::{Encoding, Format};
use mlt_core::PropKind;
use tilejson::{TileJSON, VectorLayer};

use crate::plan::{Evaluator, ExprTallies, Plan};
use crate::props::KeyNames;
use crate::source::{Crs, FeatureBatch, Geometry};
use crate::{
    DedupIndex, EncodeSettings, EncodedTile, FeatureGeom, LayerInfo, LayerSliceErrors, LayerStats,
    Renderer, Seq, SliceErrors, SortBuffer, Summary, TileEncoder, TileFormat, TileGenResult,
    TileOrder, TileRecords, project,
};

/// Renders feature batches into sort records, for one thread.
pub(crate) struct FeatureRenderer<'p> {
    renderer: Renderer,
    evaluator: Evaluator<'p>,
    order: TileOrder,
    features: u64,
    skipped: u64,
}

impl<'p> FeatureRenderer<'p> {
    /// A renderer of `plan`'s layers that keys records for tiles in `order`.
    pub(crate) fn new(plan: &'p Plan, order: TileOrder) -> Self {
        Self {
            renderer: Renderer::default(),
            evaluator: plan.evaluator(),
            order,
            features: 0,
            skipped: 0,
        }
    }

    /// Projects every feature of `batch` in place and pushes the records it renders into `out`.
    #[hotpath::measure]
    pub(crate) fn render_batch(
        &mut self,
        batch: &mut FeatureBatch,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let Self {
            renderer,
            evaluator,
            order,
            features,
            skipped,
        } = self;
        for (row, feature) in (batch.first_row..).zip(&mut batch.features) {
            project_geometry(batch.crs, &mut feature.geometry);
            let Some(geom) = feature_geom(&feature.geometry) else {
                *skipped += 1;
                continue;
            };
            *features += 1;
            let seq = Seq::new(batch.partition, row)?;
            evaluator.evaluate(batch.table, feature, geom, |evaluated| {
                renderer.render(*order, evaluated.layer(), seq, evaluated.feature(), out)
            })?;
        }
        Ok(())
    }

    /// What this renderer saw and could not render.
    pub(crate) fn finish(self) -> RenderTotals {
        RenderTotals {
            features: self.features,
            skipped: self.skipped,
            slice_errors: self.renderer.slice_errors,
            exprs: self.evaluator.finish(),
        }
    }
}

/// Counts and failures of one or more [`FeatureRenderer`]s.
#[derive(Default)]
pub(crate) struct RenderTotals {
    features: u64,
    skipped: u64,
    /// By layer index.
    slice_errors: Vec<SliceErrors>,
    exprs: ExprTallies,
}

impl RenderTotals {
    /// Adds another renderer's totals to these.
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

/// Decodes and projects the geometry in place; one that fails to decode stays WKB.
fn project_geometry(crs: Crs, geometry: &mut Geometry) {
    if !geometry.decode_wkb() {
        return;
    }
    match geometry {
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
}

/// The renderable view of a decoded geometry; `None` for one still in WKB.
fn feature_geom(geometry: &Geometry) -> Option<FeatureGeom<'_>> {
    match geometry {
        Geometry::Points(points) => Some(FeatureGeom::Points(points)),
        Geometry::Lines(lines) => Some(FeatureGeom::Lines(lines)),
        Geometry::Polygons(polygons) => Some(FeatureGeom::Polygons(polygons)),
        Geometry::Wkb(_) => None,
    }
}

/// Projects coordinates from `crs` into the renderer's unit square.
fn project_coords(crs: Crs, coords: &mut [Coord<f64>]) {
    match crs {
        Crs::Wgs84 => coords
            .iter_mut()
            .for_each(|c| *c = project::from_lonlat(*c)),
        Crs::WebMercator => coords
            .iter_mut()
            .for_each(|c| *c = project::from_mercator(*c)),
    }
}

/// Encodes grouped tiles of a rendered plan, shared by every encoder thread.
pub(crate) struct TileEncoding {
    infos: Vec<LayerInfo>,
    /// Only complete once every feature is rendered, since rendering interns new keys.
    names: Vec<KeyNames>,
    format: TileFormat,
    encoding: Encoding,
    order: TileOrder,
    dedup: DedupIndex,
}

impl TileEncoding {
    /// The encoding of `plan`'s tiles; build it after rendering, so every property key has a name.
    pub(crate) fn new(
        plan: &Plan,
        format: TileFormat,
        encoding: Encoding,
        order: TileOrder,
    ) -> Self {
        Self {
            infos: plan.layers.iter().map(|layer| layer.info.clone()).collect(),
            names: plan.key_names(),
            format,
            encoding,
            order,
            dedup: DedupIndex::default(),
        }
    }

    /// Encodes `batch` with `encoder`, which keeps the layer stats of every tile it encodes.
    pub(crate) fn encode(
        &self,
        encoder: &mut TileEncoder,
        batch: Vec<TileRecords>,
    ) -> TileGenResult<Vec<EncodedTile>> {
        let settings = EncodeSettings {
            layers: &self.infos,
            keys: &self.names,
            format: self.format,
            encoding: self.encoding,
            order: self.order,
        };
        encoder.encode_batch(&settings, &self.dedup, batch)
    }

    /// Fills `metadata` with the zooms, format and `vector_layers` that `encoders` produced.
    pub(crate) fn complete_metadata(&self, metadata: &mut TileJSON, encoders: &[TileEncoder]) {
        let mut stats = vec![LayerStats::default(); self.infos.len()];
        for encoder in encoders {
            for (total, layer) in stats.iter_mut().zip(&encoder.stats) {
                total.merge(layer);
            }
        }
        let zooms = stats.iter().filter_map(|s| s.zooms);
        metadata.minzoom = zooms.clone().map(|z| z.0).min();
        metadata.maxzoom = zooms.map(|z| z.1).max();
        let format = Format::from(self.format).metadata_format_value();
        metadata.other.insert("format".to_owned(), format.into());
        metadata.vector_layers = Some(
            self.infos
                .iter()
                .zip(&stats)
                .filter(|(_, stats)| stats.zooms.is_some())
                .map(|(info, stats)| {
                    let fields = stats
                        .fields
                        .iter()
                        .map(|(name, &kind)| (name.clone(), field_type(kind).to_owned()));
                    let mut layer = VectorLayer::new(info.name.clone(), fields.collect());
                    (layer.minzoom, layer.maxzoom) = stats.zooms.unzip();
                    layer
                })
                .collect(),
        );
    }
}

/// `vector_layers` field types, as `MBTiles` and `TileJSON` consumers expect them.
fn field_type(kind: PropKind) -> &'static str {
    match kind {
        PropKind::Bool => "Boolean",
        PropKind::Str => "String",
        PropKind::I8
        | PropKind::U8
        | PropKind::I32
        | PropKind::U32
        | PropKind::I64
        | PropKind::U64
        | PropKind::F32
        | PropKind::F64 => "Number",
    }
}
