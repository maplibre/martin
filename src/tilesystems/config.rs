use serde::{Deserialize, Serialize};
use tilejson::Bounds;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileSystemConfig {
    pub srid: i32,
    pub bounds: Bounds,
    pub identifier: String,
}

impl From<TileSystemConfig> for TileSystem {
    fn from(value: TileSystemConfig) -> Self {
        TileSystem::Custom(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TileSystem {
    Custom(TileSystemConfig),
    WebMercatorQuad,
}

impl TileSystem {
    pub fn is_web_mercator(&self) -> bool {
        matches!(self, TileSystem::WebMercatorQuad)
    }

    pub fn get_srid(&self) -> i32 {
        match self {
            TileSystem::Custom(ts) => ts.srid,
            TileSystem::WebMercatorQuad => 3857,
        }
    }
}

impl Default for TileSystem {
    fn default() -> Self {
        TileSystem::WebMercatorQuad
    }
}

impl From<TileSystem> for Option<TileSystemConfig> {
    fn from(val: TileSystem) -> Self {
        match val {
            TileSystem::WebMercatorQuad => None,
            TileSystem::Custom(ts) => Some(ts),
        }
    }
}

impl From<Option<TileSystemConfig>> for TileSystem {
    fn from(value: Option<TileSystemConfig>) -> Self {
        match value {
            None => TileSystem::WebMercatorQuad,
            Some(ts) => TileSystem::Custom(ts),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tilesystems::{TileSystem, TileSystemConfig};
    use indoc::indoc;
    use tilejson::Bounds;

    #[test]
    pub fn test_parse_tile_systems_config() {
        println!(
            "{}",
            serde_yaml::to_string(&TileSystem::Custom(TileSystemConfig {
                identifier: "WGS84Quad".to_string(),
                srid: 4326,
                bounds: Bounds::new(-180.0, -90.0, 180.0, 90.0)
            }))
            .unwrap()
        );

        let configs: Vec<TileSystem> = serde_yaml::from_str(indoc! {"
            - type: WebMercatorQuad
            - type: Custom
              identifier: WGS84Quad
              srid: 4326
              bounds: [-180, -90, 180, 90]
        "})
        .unwrap();

        assert_eq!(
            configs,
            vec![
                TileSystem::WebMercatorQuad,
                TileSystem::Custom(TileSystemConfig {
                    identifier: "WGS84Quad".to_string(),
                    srid: 4326,
                    bounds: Bounds::new(-180.0, -90.0, 180.0, 90.0)
                })
            ]
        );

        let maybe_config: Option<Vec<TileSystem>> = serde_yaml::from_str("").unwrap();
        assert!(maybe_config.is_none());
        let maybe_configs: Option<Vec<TileSystem>> = serde_yaml::from_str(
            "
            - type: WebMercatorQuad
            - type: Custom
              identifier: WGS84Quad
              srid: 4326
              bounds: [-180, -90, 180, 90]
        ",
        )
        .unwrap();
        assert!(maybe_configs.is_some());
        assert_eq!(maybe_configs.unwrap().len(), 2);

        let config: TileSystem = serde_yaml::from_str(indoc! {"\
            type: Custom
            identifier: WGS84Quad
            srid: 4326
            bounds: [-180, -90, 180, 90]
        "})
        .unwrap();
        assert_eq!(
            config,
            TileSystem::Custom(TileSystemConfig {
                identifier: "WGS84Quad".to_string(),
                srid: 4326,
                bounds: Bounds::new(-180.0, -90.0, 180.0, 90.0)
            })
        );
    }
}
