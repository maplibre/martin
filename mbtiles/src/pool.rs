use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Pool, Sqlite, SqlitePool};

use crate::errors::MbtResult;
use crate::{MbtType, Mbtiles, Metadata};

#[derive(Clone, Debug)]
pub struct MbtilesPool {
    mbtiles: Mbtiles,
    pool: Pool<Sqlite>,
}

impl MbtilesPool {
    #[deprecated(note = "Use `MbtilesPool::open_readonly` instead")]
    #[doc(hidden)]
    pub async fn new<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        Self::open_readonly(filepath).await
    }

    /// Open a `MBTiles` file in read-only mode.
    pub async fn open_readonly<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        let mbtiles = Mbtiles::new(filepath)?;
        let opt = SqliteConnectOptions::new()
            .filename(mbtiles.filepath())
            .read_only(true);
        let pool = SqlitePool::connect_with(opt).await?;
        Ok(Self { mbtiles, pool })
    }

    /// Get the metadata of the `MBTiles` file.
    ///
    /// See [`Metadata`] for more information.
    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_metadata(&mut *conn).await
    }

    /// Detect the type of the `MBTiles` file.
    ///
    /// See [`MbtType`] for more information.
    pub async fn detect_type(&self) -> MbtResult<MbtType> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.detect_type(&mut *conn).await
    }

    /// Get a tile from the pool
    ///
    /// See [`MbtilesPool::get_tile_and_hash`] if you do need the tiles' hash.
    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_tile(&mut *conn, z, x, y).await
    }

    /// Get a tile from the pool
    ///
    /// See [`MbtilesPool::get_tile`] if you don't need the tiles' hash.
    pub async fn get_tile_and_hash(
        &self,
        mbt_type: MbtType,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<(Vec<u8>, String)>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .get_tile_and_hash(&mut conn, mbt_type, z, x, y)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_invalid_type() {
        let pool = MbtilesPool::open_readonly("../tests/fixtures/mbtiles/webp.mbtiles")
            .await
            .unwrap();
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata,@r#"
        id: webp
        tile_info:
          format: webp
          encoding: ""
        layer_type: baselayer
        tilejson:
          tilejson: 3.0.0
          tiles: []
          bounds:
            - -180
            - -85.05113
            - 180
            - 85.05113
          center:
            - 0
            - 0
            - 0
          maxzoom: 0
          minzoom: 0
          name: ne2sr
          format: webp
        "#);
        // invalid type => cannot hash properly, but can get tile
        assert!(pool.detect_type().await.is_err());
        let t1 = pool.get_tile(0, 0, 0).await.unwrap().unwrap();
        assert!(!t1.is_empty());
        // this is an access and then md5 hash => should not fail
        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::Flat, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        assert_eq!(
            h2.to_uppercase(),
            "9B10637442EB23B9A1D14606C4D1799E".to_string()
        );
        for error_types in [
            MbtType::FlatWithHash,
            MbtType::Normalized { hash_view: false },
            MbtType::Normalized { hash_view: true },
        ] {
            assert!(pool.get_tile_and_hash(error_types, 0, 0, 0).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_normalized() {
        let pool = MbtilesPool::open_readonly(
            "../tests/fixtures/mbtiles/geography-class-png-no-bounds.mbtiles",
        )
        .await
        .unwrap();
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata,@r#"
        id: geography-class-png-no-bounds
        tile_info:
          format: png
          encoding: ""
        tilejson:
          tilejson: 3.0.0
          tiles: []
          description: "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. "
          legend: "<div style=\"text-align:center;\">\n\n<div style=\"font:12pt/16pt Georgia,serif;\">Geography Class</div>\n<div style=\"font:italic 10pt/16pt Georgia,serif;\">by MapBox</div>\n\n<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAJYAAACWCAYAAAA8AXHiAAAABHNCSVQICAgIfAhkiAAAAAlwSFlzAAAEUwAABFMBAq/upQAAABl0RVh0U29mdHdhcmUAd3d3Lmlua3NjYXBlLm9yZ5vuPBoAACIsSURBVHja7Z0JvI1V98dlSlHSICWSBpkvSmmSEEoyh9JAMke9ikb0Ns80Dxr16m2eVDIklShRqSgJlfImUWmS7P/6Pp19/vvuu5/nPGfAde9en8/6uPc49wx7/5611/qt4SmhlCrh1Wuu1S+CVw8srx5YXj2w/CLkW5ASJSqI9hVd2DgvT7Vt3ZoHh1jP2UP0fNF19erU4YFPRXv69fPAigOwA8qXL79g1tSpatjgwZvk926O55woukq0pF8zD6x0wHVClcqVn144b566dNSojfL7cdb/lxS9x6+VB1a6wGorWrbmfvvNWr54sRrcv//v8nsjG3x+rTyw0gaW9rsa1K+/cNXy5aprp05r5fea9nO8emClDSztsB91xBErVn/9tTq+VauV8ntlDywPrKyBlfi9Rvt27X4AXEc2a7ZIft/JA8sDK2tgJR6r37tXr1+/W7ZM1ahefZb8fpJfKw+sdEC1o2jvkP87aviQIRsenTCBX4b79fLASgdYbUTvj/j/Dj27d//bA8sDKy6g9hG9WHT17rvtxgN3iXZ2kaAifUSH+XXzwNocQNzBr4MHllcPLK8eWMXjyCMvWM6vhQdWroHVUHSiXwsPrFwDq6PoR34tPLByDazLRL/3a+GBlWtgPe6B5YG1OYD1juh3onv69fDAyiWwFor+LHq4Xw8PrFwC60tRfjjdr4cHVq5AVVH0mwSwbvJr4oEVV3cRPUqUTpyholeJThCdLDp/3rx5q3fffXdApdq0afOXPLZYdKboJNFbREeKniF6vOhBfj2LN7AOFh2RAAhgCZVnn31W7bzzzgGwDj30UBVDloneJtpGdHsPrKKtZURbJizMEpWGXHHFFapGjRoBsOrUqaPSlPWiz4meLbqXB1bR0WNFnxT9KR00/PXXX2rFihVq4cKF6tRTT1UNGzYMgMWRuGrVquRz0pRNovNELxQt74G1bWoT0Slxd/zqq69WrVu3Vo8++mjw+5NPPqnKlCmjmjVrpk466SSVl5cXAKts2bJq6tSpwXOGDRumdthhBzV06NDg9x9//FHdcsstasmSWAZxVcKfK+uBtW1oLdEnEtbBKRs3blS33XYbjrj64osvgsdefPFF9eCDD6pFixYln7Np0z8v0a1btySwdtppJzV79uzg8d9++y0A0dKlS4PfP//8c9W2bVu15557Jq2ZOP6pLBv+GBRGSQ+swqnVRKlR3+javTlz5qhrr702+TvH26233qp++OGHlKbl9NNPTwILBYCpjlHkjz/+UNWqVVMVK1ZUn376aaq3+Vi0owdW4dHdRW9mH8N2DEuCb3TWWWcFm52OrFu3LjjqTGANHz48vlMlVu/9999Pgq1Lly4BoP/888+wP5kj2sIDa+tq64SvUkCwEOeee25wpGnrkUq+/vpr9cYbb6j//ve/aty4cWrUqFGqQ4cO6oILLsgHrL322it4bfwyrNcrr7yiPvjgg1jO/KuvvqoOOeQQ9e6776Z66p2i5TywtqyWFr0mzI/ieOP4wblev3595JEFkC688EJVt25dVaVKFXXUUUep7t27B1bp+uuvD16DI9QE1gEHHKBuv/12dckllwRWEL+qXr16atdddw38sYceekj973//i2XNeK+nn3467CkfJvg2D6wtoNVFZxcIsVatCiwLDjXy009udmH16tXqkUceUaecckoABAjPsWPHBseVS8aMGaMmTJiQD1hhXBaA5rUBS6VKlVTTpk0DDozX1oGAKX///be6//77A44sIpJcn2D1PbA2ozYQ/dZeefyVmjVrBhsaZim++uor1bdvX7XLLruoTp06BWDRfFSUYIGIGE1g7bvvvgEoUjnwM2bMUOeff7466KCDVIMGDYLXcYl+LS4K6Ap9fNs8rQfW5iM614VtCmSmS77//vvgWKtcuXJgmX755Ze0nPfDDjssiChtH+ubb75J63WmTZsWWMcjjjhCzZw50/kcXrN+/foBbxYi94qW8sDKnXazoz4sAg70kCFDQqO5yy67TO2xxx5qxIgRsWgFl+B7ffnll/mABZf11ltvZfR6Tz31lDr44IPV8ccfr957770C/w/wNQEblrrcFpz6bQFUPTBMrOjkyZOT5OQLL7wQRFdEcjbgcLohKfv3769WrlypspH99tsv2GwTWOgDDzyQ8Wty1HEUV69ePaAfPvvsM+fzOEb5zhCwRKDGMf9qIvfpgZWhtsKF2rBhQwCiChUqBNFeq1atnLk6rFKLFi3UiSeemGTU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\">\n</div>"
          maxzoom: 1
          minzoom: 0
          name: Geography Class
          template: "{{#__location__}}{{/__location__}}{{#__teaser__}}<div style=\"text-align:center;\">\n\n<img src=\"data:image/png;base64,{{flag_png}}\" style=\"-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;\"><br>\n<strong>{{admin}}</strong>\n\n</div>{{/__teaser__}}{{#__full__}}{{/__full__}}"
          version: 1.0.0
        "#);
        assert_eq!(
            pool.detect_type().await.unwrap(),
            MbtType::Normalized { hash_view: false }
        );
        let t1 = pool.get_tile(0, 0, 0).await.unwrap().unwrap();
        assert!(!t1.is_empty());

        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::Normalized { hash_view: false }, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        assert_eq!(
            h2.to_uppercase(),
            "1578FDCA522831A6435F7795586C235B".to_string()
        );

        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Flat, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t1);
        assert_eq!(h3.to_uppercase(), h2.to_uppercase());
        for error_types in [
            MbtType::FlatWithHash,
            MbtType::Normalized { hash_view: true },
        ] {
            assert!(pool.get_tile_and_hash(error_types, 0, 0, 0).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_flat_with_hash() {
        let pool =
            MbtilesPool::open_readonly("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles")
                .await
                .unwrap();
        let metadata = pool.get_metadata().await.unwrap();
        insta::assert_yaml_snapshot!(metadata,@r#"
        id: zoomed_world_cities
        tile_info:
          format: mvt
          encoding: gzip
        layer_type: overlay
        tilejson:
          tilejson: 3.0.0
          tiles: []
          vector_layers:
            - id: cities
              fields:
                name: String
              description: ""
              maxzoom: 6
              minzoom: 0
          bounds:
            - -123.12359
            - -37.818085
            - 174.763027
            - 59.352706
          center:
            - -75.9375
            - 38.788894
            - 6
          description: Major cities from Natural Earth data
          maxzoom: 6
          minzoom: 0
          name: Major cities from Natural Earth data
          version: "2"
          format: pbf
        json:
          tilestats:
            layerCount: 1
            layers:
              - attributeCount: 1
                attributes:
                  - attribute: name
                    count: 68
                    type: string
                    values:
                      - Addis Ababa
                      - Amsterdam
                      - Athens
                      - Atlanta
                      - Auckland
                      - Baghdad
                      - Bangalore
                      - Bangkok
                      - Beijing
                      - Berlin
                      - Bogota
                      - Buenos Aires
                      - Cairo
                      - Cape Town
                      - Caracas
                      - Casablanca
                      - Chengdu
                      - Chicago
                      - Dakar
                      - Denver
                      - Dubai
                      - Geneva
                      - Hong Kong
                      - Houston
                      - Istanbul
                      - Jakarta
                      - Johannesburg
                      - Kabul
                      - Kiev
                      - Kinshasa
                      - Kolkata
                      - Lagos
                      - Lima
                      - London
                      - Los Angeles
                      - Madrid
                      - Manila
                      - Melbourne
                      - Mexico City
                      - Miami
                      - Monterrey
                      - Moscow
                      - Mumbai
                      - Nairobi
                      - New Delhi
                      - New York
                      - Paris
                      - Rio de Janeiro
                      - Riyadh
                      - Rome
                      - San Francisco
                      - Santiago
                      - Seoul
                      - Shanghai
                      - Singapore
                      - Stockholm
                      - Sydney
                      - São Paulo
                      - Taipei
                      - Tashkent
                      - Tehran
                      - Tokyo
                      - Toronto
                      - Vancouver
                      - Vienna
                      - "Washington, D.C."
                      - Ürümqi
                      - Ōsaka
                count: 68
                geometry: Point
                layer: cities
        agg_tiles_hash: D4E1030D57751A0B45A28A71267E46B8
        "#);
        assert_eq!(pool.detect_type().await.unwrap(), MbtType::FlatWithHash);
        let t1 = pool.get_tile(0, 0, 0).await.unwrap().unwrap();
        assert!(!t1.is_empty());

        let (t2, h2) = pool
            .get_tile_and_hash(MbtType::FlatWithHash, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t2, t1);
        assert_eq!(
            h2.to_uppercase(),
            "1578FDCA522831A6435F7795586C235B".to_string()
        );

        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Normalized { hash_view: false }, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t1);
        assert_eq!(h3.to_uppercase(), h2.to_uppercase());

        let (t3, h3) = pool
            .get_tile_and_hash(MbtType::Flat, 0, 0, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t3, t1);
        assert_eq!(h3.to_uppercase(), h2.to_uppercase());
        // does not work as view does not exist
        assert!(
            pool.get_tile_and_hash(MbtType::Normalized { hash_view: true }, 0, 0, 0)
                .await
                .is_err()
        );
    }
}
