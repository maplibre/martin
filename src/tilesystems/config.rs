use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tilejson::Bounds;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileSystemConfig {
    pub srid: i32,
    pub bounds: Bounds,
}

pub type TileSystemsConfig = HashMap<String, TileSystemConfig>;

#[cfg(test)]
mod tests {
    use crate::config::tests::assert_config;
    use crate::pg::PgConfig;
    use crate::test_utils::some;
    use crate::tilesystems::TileSystemConfig;
    use crate::OneOrMany::One;
    use crate::{BoolOrObject, Config};
    use indoc::indoc;
    use std::collections::HashMap;
    use tilejson::Bounds;

    #[test]
    pub fn test_parse_tile_systems_config() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
            tile_systems:
              my_custom_tiling:
                srid: 4326
                bounds: [-180, -90, 180, 90]
        "},
            &Config {
                postgres: Some(One(PgConfig {
                    connection_string: some("postgresql://postgres@localhost/db"),
                    auto_publish: Some(BoolOrObject::Bool(true)),
                    ..Default::default()
                })),
                tile_systems: Some(HashMap::from([(
                    "my_custom_tiling".to_string(),
                    TileSystemConfig {
                        srid: 4326,
                        bounds: Bounds::new(-180.0, -90.0, 180.0, 90.0),
                    },
                )])),
                ..Default::default()
            },
        );
    }
}
