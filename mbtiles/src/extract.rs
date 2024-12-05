use crate::{MbtError, MbtResult, Mbtiles};
use futures::{FutureExt, StreamExt, TryStreamExt};
use log::{debug, error};
use martin_tile_utils::{decode_brotli, decode_gzip, Encoding, Format, TileCoord};
use object_store::aws::{AmazonS3, AmazonS3Builder, AmazonS3ConfigKey};
use object_store::azure::{AzureConfigKey, MicrosoftAzure, MicrosoftAzureBuilder};
use object_store::gcp::{GoogleCloudStorage, GoogleCloudStorageBuilder, GoogleConfigKey};
use object_store::local::LocalFileSystem;
#[cfg(test)]
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::{
    Attribute, AttributeValue, Attributes, ObjectStore, ObjectStoreScheme, PutMode, PutOptions,
    PutPayload,
};
use sqlx::query_as;
use std::collections::HashMap;
use std::future::ready;
use std::path::PathBuf;
use std::str::FromStr;
use strfmt::strfmt;
use url::Url;

trait DecodeBytes {
    fn decode_bytes(&self, data: Vec<u8>) -> MbtResult<Vec<u8>>;
}

impl DecodeBytes for Encoding {
    fn decode_bytes(&self, data: Vec<u8>) -> MbtResult<Vec<u8>> {
        match self {
            Self::Internal | Self::Uncompressed => Ok(data),
            Self::Gzip => Ok(decode_gzip(&data)?),
            Self::Brotli => Ok(decode_brotli(&data)?),
            Self::Zstd | Self::Zlib => {
                error!("No decompressor implemented for encoding: {:?}", self);
                Err(MbtError::UnsupportedEncodingForDecompression)
            }
        }
    }
}

impl DecodeBytes for Option<Encoding> {
    fn decode_bytes(&self, data: Vec<u8>) -> MbtResult<Vec<u8>> {
        match self {
            Some(encoding) => encoding.decode_bytes(data),
            None => Ok(data),
        }
    }
}

#[derive(Debug)]
struct TileRecord {
    zoom_level: Option<i64>,
    tile_row: Option<i64>,
    tile_column: Option<i64>,
    tile_data: Option<Vec<u8>>,
}

impl TileRecord {
    fn tile_coord(&self) -> Option<TileCoord> {
        self.zoom_level
            .zip(self.tile_column)
            .zip(self.tile_row)
            .map(|((zoom_level, tile_column), tile_row)| TileCoord {
                z: u8::try_from(zoom_level).expect("zoom level out of range"),
                x: u32::try_from(tile_column).expect("tile column out of range"),
                y: u32::try_from(tile_row).expect("tile row out of range"),
            })
    }
}

pub async fn extract(
    file: PathBuf,
    output_url: &str,
    options: Vec<(String, String)>,
    concurrency: u8,
    decode: bool,
) -> MbtResult<()> {
    let mbt = Mbtiles::new(file.as_path())?;
    let tilestore_url = Url::try_from(output_url)?;
    let (tilestore, path) = TileStore::parse(&tilestore_url, options)?;
    let mut conn = mbt.open_readonly().await?;
    let tile_info = mbt.get_metadata(&mut conn).await?.tile_info;

    let decode_from = if decode && tile_info.encoding.is_encoded() {
        Some(tile_info.encoding)
    } else {
        None
    };

    let put_options = get_putoptions(
        tilestore.supports_attributes(),
        tile_info.format,
        if decode_from.is_some() {
            Encoding::Uncompressed
        } else {
            tile_info.encoding
        },
    );

    let tilewriter = TileWriter {
        store: tilestore,
        path_template: path.to_string(),
        put_options,
    };

    let rows = query_as!(
        TileRecord,
        "
    SELECT zoom_level,
        tile_column,
        tile_row,
        tile_data
    FROM tiles"
    )
    .fetch(&mut conn);

    rows.map(|record| record.map_err(MbtError::from))
        .try_filter_map(|record| ready(decode_tile_data(record, decode_from)))
        // limit the concurrency to avoid overwhelming the object store
        .try_for_each_concurrent(concurrency as usize, |(coord, tile_data)| {
            tilewriter.write_tile(coord, tile_data).boxed()
        })
        .await?;

    Ok(())
}

fn decode_tile_data(
    tile_record: TileRecord,
    decode_from_encoding: Option<Encoding>,
) -> MbtResult<Option<(TileCoord, Vec<u8>)>> {
    let coord = tile_record.tile_coord();
    if let (Some(tile_data), Some(coord)) = (tile_record.tile_data, coord) {
        match decode_from_encoding.decode_bytes(tile_data) {
            Ok(decoded_tile_data) => Ok(Some((coord, decoded_tile_data))),
            Err(e) => {
                error!("Error decoding tile: {:?}", e);
                Err(e)
            }
        }
    } else {
        Ok(None)
    }
}

fn get_putoptions(supports_attributes: bool, format: Format, encoding: Encoding) -> PutOptions {
    let mut attributes = Attributes::new();
    if supports_attributes {
        attributes.insert(
            Attribute::ContentType,
            AttributeValue::from(format.content_type().to_string()),
        );
        if let Some(content_encoding) = encoding.content_encoding() {
            attributes.insert(
                Attribute::ContentEncoding,
                AttributeValue::from(content_encoding.to_string()),
            );
        }
    }
    PutOptions {
        mode: PutMode::Overwrite,
        attributes,
        ..Default::default()
    }
}

enum TileStore {
    Local(LocalFileSystem),
    S3(AmazonS3),
    Azure(MicrosoftAzure),
    Gcp(GoogleCloudStorage),
    #[cfg(test)]
    Memory(InMemory),
}

macro_rules! builder_from_env_with_opts {
    ($builder:ty, $url:expr, $options:expr, $keytype:ty) => {{
        let builder = $options.into_iter().fold(
            <$builder>::from_env().with_url($url.to_string()),
            |builder, (key, value)| match <$keytype>::from_str(key.as_ref()) {
                Ok(k) => builder.with_config(k, value),
                Err(_) => builder,
            },
        );
        builder.build()?
    }};
}

impl TileStore {
    fn parse(url: &Url, options: Vec<(String, String)>) -> MbtResult<(Self, Path)> {
        let (scheme, path) =
            ObjectStoreScheme::parse(url).map_err(|_| MbtError::ObjectStoreParseError)?;
        let path = Path::parse(path)?;

        let tilestore = match scheme {
            ObjectStoreScheme::Local => TileStore::Local(LocalFileSystem::new()),
            ObjectStoreScheme::AmazonS3 => TileStore::S3(builder_from_env_with_opts!(
                AmazonS3Builder,
                url,
                options,
                AmazonS3ConfigKey
            )),
            ObjectStoreScheme::MicrosoftAzure => TileStore::Azure(builder_from_env_with_opts!(
                MicrosoftAzureBuilder,
                url,
                options,
                AzureConfigKey
            )),
            ObjectStoreScheme::GoogleCloudStorage => TileStore::Gcp(builder_from_env_with_opts!(
                GoogleCloudStorageBuilder,
                url,
                options,
                GoogleConfigKey
            )),
            _ => return Err(MbtError::UnsupportedObjectStoreScheme),
        };
        Ok((tilestore, path))
    }

    fn supports_attributes(&self) -> bool {
        match self {
            // local filesystem does yet not support setting attributes
            // and will return a not-implemented error when any are set.
            TileStore::Local(_) => false,
            _ => true,
        }
    }

    async fn put_opts(&self, path: &Path, data: Vec<u8>, opts: PutOptions) -> MbtResult<()> {
        let payload = PutPayload::from(data);
        match self {
            TileStore::Local(fs) => fs.put_opts(path, payload, opts),
            TileStore::S3(s3) => s3.put_opts(path, payload, opts),
            TileStore::Azure(azure) => azure.put_opts(path, payload, opts),
            TileStore::Gcp(gcp) => gcp.put_opts(path, payload, opts),
            #[cfg(test)]
            TileStore::Memory(mem) => mem.put_opts(path, payload, opts),
        }
        .await?;
        Ok(())
    }
}

struct TileWriter {
    store: TileStore,
    path_template: String,
    put_options: PutOptions,
}

impl TileWriter {
    async fn write_tile(&self, tile_coord: TileCoord, tile_data: Vec<u8>) -> MbtResult<()> {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), tile_coord.x.to_string());
        vars.insert("y".to_string(), tile_coord.y.to_string());
        vars.insert("z".to_string(), tile_coord.z.to_string());
        let path = strfmt(self.path_template.as_ref(), &vars)
            .map_err(|_| MbtError::ObjectStoreKeyFormatError)?;
        debug!("Writing tile {} to {}", tile_coord, path);
        self.store
            .put_opts(&Path::from(path), tile_data, self.put_options.clone())
            .await?;
        Ok(())
    }
}

impl From<TileWriter> for TileStore {
    fn from(writer: TileWriter) -> Self {
        writer.store
    }
}

#[cfg(test)]
mod tests {
    use crate::extract::{get_putoptions, TileStore, TileWriter};
    use martin_tile_utils::{Encoding, Format, TileCoord};
    use object_store::memory::InMemory;
    use object_store::path::Path;
    use object_store::{Attribute, AttributeValue, ObjectStore};

    #[actix_rt::test]
    async fn write_tile() {
        let tile_coord = TileCoord { z: 1, x: 2, y: 3 };
        let tile_data = vec![0, 1, 2, 3];
        let writer = TileWriter {
            store: TileStore::Memory(InMemory::new()),
            path_template: "tiles/{z}/{x}/{y}.pbf".to_string(),
            put_options: get_putoptions(true, Format::Webp, Encoding::Brotli),
        };
        let result = writer.write_tile(tile_coord, tile_data.clone()).await;
        assert!(result.is_ok());

        let store: TileStore = writer.into();
        match store {
            TileStore::Memory(inmemory) => {
                let path = Path::from("tiles/1/2/3.pbf");
                let getresult = inmemory
                    .get(&path)
                    .await
                    .expect("Failed to get tile from in-memory store");

                assert_eq!(
                    getresult.attributes.get(&Attribute::ContentEncoding),
                    Some(&AttributeValue::from(
                        Encoding::Brotli.content_encoding().unwrap()
                    ))
                );

                assert_eq!(
                    getresult.attributes.get(&Attribute::ContentType),
                    Some(&AttributeValue::from(Format::Webp.content_type()))
                );

                let data = getresult
                    .bytes()
                    .await
                    .expect("Failed to get tile bytes from in-memory store")
                    .to_vec();
                assert_eq!(data, tile_data);
            }
            _ => panic!("Unexpected tilestore variant"),
        }
    }

    #[test]
    fn parse_local_store_url() {
        let url = "file:///tmp/tiles/{z}/{x}/{y}.pbf";
        let options = vec![];
        let (store, path) = TileStore::parse(&url.parse().unwrap(), options).unwrap();
        assert!(matches!(store, TileStore::Local(_)));
        // unix-filesystem paths are always relative to /
        assert_eq!(path.to_string(), "tmp/tiles/{z}/{x}/{y}.pbf");
    }

    #[test]
    fn parse_s3_store_url() {
        let url = "s3://bucket-name/tiles/{z}/{x}/{y}.pbf";
        let options = vec![];
        let (store, path) = TileStore::parse(&url.parse().unwrap(), options).unwrap();
        assert!(matches!(store, TileStore::S3(_)));
        assert_eq!(path.to_string(), "tiles/{z}/{x}/{y}.pbf");
    }

    #[test]
    fn parse_gcp_store_url() {
        let url = "gs://bucket-name/tiles/{z}/{x}/{y}.pbf";
        let options = vec![];
        let (store, path) = TileStore::parse(&url.parse().unwrap(), options).unwrap();
        assert!(matches!(store, TileStore::Gcp(_)));
        assert_eq!(path.to_string(), "tiles/{z}/{x}/{y}.pbf");
    }

    #[test]
    fn parse_azore_store_url() {
        let url = "az://container/tiles/{z}/{x}/{y}.pbf";
        let options = vec![("account_name".to_string(), "myaccount".to_string())];
        let (store, path) = TileStore::parse(&url.parse().unwrap(), options).unwrap();
        assert!(matches!(store, TileStore::Azure(_)));
        assert_eq!(path.to_string(), "tiles/{z}/{x}/{y}.pbf");
    }
}
