mod commands;
mod geometry_encoding;
mod tag_builder;
mod tile_value;
#[rustfmt::skip]
pub mod vector_tile;

use geometry_encoding::encode_geom;
use tag_builder::TagsBuilder;

pub struct LayerBuilder {
    name: String,
    tag_builder: TagsBuilder<String>,
    features: Vec<vector_tile::tile::Feature>,
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
            geojson::Value::Point(_) => vector_tile::tile::GeomType::Point,
            geojson::Value::LineString(_) => vector_tile::tile::GeomType::Linestring,
            geojson::Value::Polygon(_) => vector_tile::tile::GeomType::Polygon,
            _ => vector_tile::tile::GeomType::Unknown,
        } as i32);

        let mut tags = Vec::new();
        if feature.properties.is_some() {
            for property in feature.properties.as_ref().unwrap().iter() {
                let (key, val) = (property.0, property.1);
                let (key_idx, val_idx) = self.tag_builder.insert(key.clone(), val.clone().into());
                tags.push(key_idx);
                tags.push(val_idx);
            }
        }

        self.features.push(vector_tile::tile::Feature {
            id,
            tags,
            r#type,
            geometry,
        });
    }

    pub fn build(self) -> vector_tile::tile::Layer {
        let (keys, values) = self.tag_builder.into_tags();
        let values = values.into_iter().map(|e| e.into()).collect();
        vector_tile::tile::Layer {
            name: self.name,
            features: self.features,
            version: 2,
            extent: Some(self.extent),
            keys,
            values,
        }
    }
}
