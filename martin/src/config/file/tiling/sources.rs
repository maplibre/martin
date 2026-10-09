use std::num::NonZeroU8;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Prefetch(NonZeroU8);

impl Prefetch {
    #[must_use]
    pub fn tiles_per_side(self) -> u8 {
        self.0.get()
    }
}

#[cfg(test)]
#[cfg(feature = "postgres")]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use indoc::indoc;

    use crate::config::file::tiling::Layers;
    use crate::config::file::{CollectUnrecognizedKeys as _, Config, parse_config};

    fn parse(yaml: &str) -> Config {
        parse_config(yaml, &HashMap::new(), Path::new("martin.yaml")).expect("parses")
    }

    fn rejection(yaml: &str) -> String {
        parse_config(yaml, &HashMap::new(), Path::new("martin.yaml"))
            .expect_err("must not parse")
            .to_string()
    }

    #[test]
    fn a_pg_table_with_layers_is_tiled_by_the_engine() {
        let config = parse(indoc! {"
            postgres:
              connection_string: postgres://localhost/db
              prefetch: 2
              tables:
                roads:
                  schema: public
                  table: roads
                  srid: 4326
                  geometry_column: geom
                  prefetch: 1
                  layers:
                    roads: { simplify: 1, min_size: 0.5 }
                cities:
                  schema: public
                  table: cities
                  srid: 4326
                  geometry_column: geom
        "});
        let pg = &config.postgres[0];
        let tables = pg.tables.as_ref().unwrap();
        assert_eq!(pg.prefetch.unwrap().tiles_per_side(), 2);
        assert_eq!(tables["roads"].prefetch.unwrap().tiles_per_side(), 1);
        assert_eq!(tables["cities"].layers, None);
        let expected: Layers =
            serde_saphyr::from_str("roads: { simplify: 1, min_size: 0.5 }").unwrap();
        assert_eq!(tables["roads"].layers.as_deref(), Some(&expected));
        assert!(config.get_unrecognized_keys().is_empty());
    }

    #[test]
    fn a_pg_table_names_its_layer_one_way_only() {
        insta::assert_snapshot!(rejection(indoc! {"
            postgres:
              connection_string: postgres://localhost/db
              tables:
                roads:
                  schema: public
                  table: roads
                  srid: 4326
                  geometry_column: geom
                  layer_id: streets
                  layers:
                    roads: {}
        "}), @"Unable to parse YAML in config file martin.yaml: table `roads`: `layer_id` names the layer PostGIS tiles; with `layers`, each layer is named by its key, so drop `layer_id` at line 4, column 5");
    }
}
