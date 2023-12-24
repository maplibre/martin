mod file_pmtiles;
mod http_pmtiles;

pub use file_pmtiles::PmtFileSource;
pub use http_pmtiles::PmtHttpSource;

macro_rules! impl_pmtiles_source {
    ($name: ident, $backend: ty, $cache: ty, $path: ty, $display_path: path, $err: ident) => {
        #[derive(Clone)]
        pub struct $name {
            id: String,
            path: $path,
            pmtiles: Arc<AsyncPmTilesReader<$backend, $cache>>,
            tilejson: TileJSON,
            tile_info: TileInfo,
        }

        impl Debug for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "{} {{ id: {}, path: {:?} }}",
                    stringify!($name),
                    self.id,
                    self.path
                )
            }
        }

        impl $name {
            async fn new_int(
                id: String,
                path: $path,
                reader: AsyncPmTilesReader<$backend, $cache>,
            ) -> FileResult<Self> {
                let hdr = &reader.get_header();

                if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
                    return Err($err(
                        format!(
                            "Format {:?} and compression {:?} are not yet supported",
                            hdr.tile_type, hdr.tile_compression
                        ),
                        path,
                    ));
                }

                let format = match hdr.tile_type {
                    TileType::Mvt => TileInfo::new(
                        Format::Mvt,
                        match hdr.tile_compression {
                            Compression::None => Encoding::Uncompressed,
                            Compression::Unknown => {
                                warn!(
                                    "MVT tiles have unknown compression in file {}",
                                    $display_path(&path)
                                );
                                Encoding::Uncompressed
                            }
                            Compression::Gzip => Encoding::Gzip,
                            Compression::Brotli => Encoding::Brotli,
                            Compression::Zstd => Encoding::Zstd,
                        },
                    ),
                    // All these assume uncompressed data (validated above)
                    TileType::Png => Format::Png.into(),
                    TileType::Jpeg => Format::Jpeg.into(),
                    TileType::Webp => Format::Webp.into(),
                    TileType::Unknown => return Err($err("Unknown tile type".to_string(), path)),
                };

                let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
                    warn!(
                        "{e:?}: Unable to parse metadata for {}",
                        $display_path(&path)
                    );
                    hdr.get_tilejson(Vec::new())
                });

                Ok(Self {
                    id,
                    path,
                    pmtiles: Arc::new(reader),
                    tilejson,
                    tile_info: format,
                })
            }
        }

        #[async_trait]
        impl Source for $name {
            fn get_id(&self) -> &str {
                &self.id
            }

            fn get_tilejson(&self) -> &TileJSON {
                &self.tilejson
            }

            fn get_tile_info(&self) -> TileInfo {
                self.tile_info
            }

            fn clone_source(&self) -> Box<dyn Source> {
                Box::new(self.clone())
            }

            async fn get_tile(
                &self,
                xyz: &TileCoord,
                _url_query: &Option<UrlQuery>,
            ) -> MartinResult<TileData> {
                // TODO: optimize to return Bytes
                if let Some(t) = self
                    .pmtiles
                    .get_tile(xyz.z, u64::from(xyz.x), u64::from(xyz.y))
                    .await
                {
                    Ok(t.to_vec())
                } else {
                    trace!(
                        "Couldn't find tile data in {}/{}/{} of {}",
                        xyz.z,
                        xyz.x,
                        xyz.y,
                        &self.id
                    );
                    Ok(Vec::new())
                }
            }
        }
    };
}

pub(crate) use impl_pmtiles_source;
