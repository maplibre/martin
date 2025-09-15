mod geometry_encoding;

use geometry_encoding::encode_geom;
use geozero::mvt::{TagsBuilder, TileValue, tile};

pub struct LayerBuilder {
    name: String,
    tag_builder: TagsBuilder<String>,
    features: Vec<tile::Feature>,
    extent: u32,
}

impl LayerBuilder {
    pub fn new(name: String, extent: u32) -> Self {
        Self {
            name,
            tag_builder: TagsBuilder::new(),
            features: Vec::new(),
            extent,
        }
    }

    pub fn add_feature(&mut self, feature: &geojson::Feature) {
        let geometry = encode_geom(feature.geometry.as_ref().unwrap());
        // TODO: look at mvt spec to figure out id requirements
        let id = Some(
            feature
                .id
                .as_ref()
                .map(|id| match id {
                    geojson::feature::Id::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                })
                .unwrap_or(0),
        );

        // TODO: review
        let r#type = Some(match feature.geometry.as_ref().unwrap().value {
            geojson::Value::Point(_) => tile::GeomType::Point,
            geojson::Value::LineString(_) => tile::GeomType::Linestring,
            geojson::Value::Polygon(_) => tile::GeomType::Polygon,
            _ => tile::GeomType::Unknown,
        } as i32);

        let mut tags = Vec::new();
        if feature.properties.is_some() {
            for property in feature.properties.as_ref().unwrap().iter() {
                let (key, val) = (property.0, property.1);
                let (key_idx, val_idx) = self
                    .tag_builder
                    .insert(key.clone(), tilevalue_from_json(val.clone()));
                tags.push(key_idx);
                tags.push(val_idx);
            }
        }

        self.features.push(tile::Feature {
            id,
            tags,
            r#type,
            geometry,
        });
    }

    pub fn build(self) -> tile::Layer {
        let (keys, values) = self.tag_builder.into_tags();
        let values = values.into_iter().map(|e| e.into()).collect();
        tile::Layer {
            name: self.name,
            features: self.features,
            version: 2,
            extent: Some(self.extent),
            keys,
            values,
        }
    }
}

fn tilevalue_from_json(value: serde_json::Value) -> TileValue {
    match value {
        serde_json::Value::String(s) => TileValue::Str(s),
        serde_json::Value::Number(n) => {
            if n.is_f64() {
                TileValue::Double(n.as_f64().unwrap())
            } else if n.is_i64() {
                TileValue::Int(n.as_i64().unwrap())
            } else if n.is_u64() {
                TileValue::Uint(n.as_u64().unwrap())
            } else {
                // TODO: check
                unreachable!()
            }
        }
        serde_json::Value::Bool(b) => TileValue::Bool(b),
        _ => TileValue::Str(value.to_string()),
    }
}
