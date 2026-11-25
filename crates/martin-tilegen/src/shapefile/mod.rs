//! An ESRI shapefile as a [`FeatureSource`]: each record a feature, with the fields of its `.dbf` row as properties.
//!
//! A shapefile is a `.shp` with its `.shx`, `.dbf`, `.prj` and `.cpg` beside it, or a `.zip` holding one, whose parts
//! are unpacked into a temporary directory first. Each table of the plan reads its file in partitions of whole records.

mod prj;

use std::fs::File;
use std::io::{BufReader, Cursor, Read as _, Seek as _, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use geo::{Contains as _, Intersects as _};
use geo_types::{Coord, LineString, Polygon, Rect};
use martin_tile_utils::wgs84_to_webmercator;
use shapefile::dbase::{self, FieldIndex, FieldType, FieldValue};
use shapefile::record::traits::HasXY;
use shapefile::{PolygonRing, ReadableShape as _, Shape, ShapeType};
use tilejson::Bounds;
use tracing::info;

use crate::TileGenError;
use crate::plan::{ColumnDef, GeometryType, Plan, TableDef, ValueKind};
use crate::props::{KeyId, KeyInterner, Prop, SourceStr};
use crate::source::{
    Crs, FeatureBatch, FeatureReader, FeatureSource, Geometry, SourceError, SourceFeature, SourceResult,
};

/// Bytes of `.shp` records a partition reads at once, or one record if that is larger.
const PARTITION_BYTES: u64 = 16 << 20;
/// Features per batch sent to the render workers.
const BATCH_FEATURES: usize = 4096;
/// Length of the header of a `.shp` or `.shx` file.
const HEADER_BYTES: u64 = 100;
/// Length of the header of a `.shp` record.
const RECORD_HEADER_BYTES: u64 = 8;
/// The parts of a shapefile Martin reads, by extension.
const PARTS: [&str; 5] = ["shp", "shx", "dbf", "prj", "cpg"];

/// The smallest integer a float may not hold exactly, 2^53.
const MAX_EXACT: f64 = 9_007_199_254_740_992.0;
/// The highest latitude of Web Mercator.
const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// Why reading a shapefile failed.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ShapefileError {
    /// A file cannot be read.
    #[error("Cannot read {}: {error}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },

    /// The zip file cannot be read or unpacked.
    #[error("Cannot unpack {}: {error}", path.display())]
    Zip {
        path: PathBuf,
        #[source]
        error: zip::result::ZipError,
    },

    /// The zip file holds no shapefile, or more than one.
    #[error("{} holds {found} shapefiles; Martin reads a zip file that holds exactly one", path.display())]
    ZipShapefiles { path: PathBuf, found: usize },

    /// The shapefile has no `.prj`, so its coordinate system is unknown.
    #[error("{} has no .prj file; Martin needs it to tell EPSG:4326 from EPSG:3857", path.display())]
    NoProjection { path: PathBuf },

    /// The `.prj` describes a coordinate system Martin does not read.
    #[error("{} is in `{name}`; Martin reads shapefiles in EPSG:4326 or EPSG:3857 only", path.display())]
    UnsupportedProjection { path: PathBuf, name: String },

    /// The records are of a type no tile can hold.
    #[error("{} holds {shape_type} records, which cannot go into a tile", path.display())]
    UnsupportedShapeType { path: PathBuf, shape_type: String },

    /// The `shapefile` crate cannot read the `.shp` or `.shx`.
    #[error("Cannot read {}: {error}", path.display())]
    Shapes {
        path: PathBuf,
        #[source]
        error: shapefile::Error,
    },

    /// The `dbase` crate cannot read the `.dbf`.
    #[error("Cannot read {}: {error}", path.display())]
    Attributes {
        path: PathBuf,
        #[source]
        error: dbase::Error,
    },

    /// The `.shp` and the `.dbf` hold different numbers of records.
    #[error("{} has {shapes} shapes but {rows} rows in its .dbf file", path.display())]
    RecordCounts { path: PathBuf, shapes: usize, rows: usize },

    /// The `.shx` places a record past the end of the `.shp`.
    #[error("{} places record {record} up to byte {end}, past the end of the .shp file at byte {size}", path.display())]
    RecordPastEnd {
        path: PathBuf,
        record: u64,
        end: u64,
        size: u64,
    },

    /// One record cannot be read.
    #[error("Cannot read record {record} of {}: {error}", path.display())]
    Record {
        path: PathBuf,
        record: u64,
        #[source]
        error: shapefile::Error,
    },
}

impl From<ShapefileError> for SourceError {
    fn from(err: ShapefileError) -> Self {
        Self::Other(Box::new(err))
    }
}

impl From<ShapefileError> for TileGenError {
    fn from(err: ShapefileError) -> Self {
        SourceError::from(err).into()
    }
}

/// A shapefile opened for reading: its coordinate system, its fields and where each record is.
#[derive(Clone)]
pub struct Shapefile(Arc<Opened>);

/// What [`Shapefile::open`] found out about a shapefile.
struct Opened {
    /// The `.shp` or `.zip` as given.
    path: PathBuf,
    /// The `.shp`, unpacked if it came zipped.
    shp: PathBuf,
    /// The `.dbf` beside it.
    dbf: PathBuf,
    /// The name of the `.dbf` text encoding in its `.cpg`, if it has one.
    encoding: Option<String>,
    crs: Crs,
    /// The type of every shape, from the header of the `.shp`.
    shape_type: ShapeType,
    fields: Vec<Field>,
    /// Where the first record of the `.dbf` starts, in bytes.
    dbf_header: u64,
    /// How many bytes each record of the `.dbf` takes.
    dbf_record: u64,
    /// Where each record is in the `.shp`, header included, in bytes.
    records: Vec<Range<u64>>,
    /// The directory the zip file was unpacked into, deleted with it.
    _unpacked: Option<tempfile::TempDir>,
}

/// One field of the `.dbf`.
struct Field {
    name: String,
    field_type: FieldType,
    /// Whether a numeric field has decimal places, so its values are floats.
    decimals: bool,
    /// Where the field starts in a record, deletion flag included, in bytes.
    offset: u64,
    /// How many bytes the field takes in a record.
    length: usize,
}

impl Field {
    /// The kind of the field's values.
    fn kind(&self) -> ValueKind {
        match self.field_type {
            FieldType::Character | FieldType::Date | FieldType::DateTime | FieldType::Memo => ValueKind::String,
            FieldType::Numeric if !self.decimals => ValueKind::Int,
            FieldType::Integer => ValueKind::Int,
            FieldType::Numeric | FieldType::Float | FieldType::Currency | FieldType::Double => ValueKind::Float,
            FieldType::Logical => ValueKind::Bool,
        }
    }
}

impl Shapefile {
    /// Opens the `.shp` at `path`, or the one `.zip` file at `path` holds, unpacking it into `temp_dir`.
    ///
    /// # Errors
    /// If a part cannot be read, the zip file does not hold exactly one shapefile, the `.prj` is missing or not in
    /// EPSG:4326 or EPSG:3857, or the records are multipatches.
    pub fn open(path: &Path, temp_dir: &Path) -> Result<Self, ShapefileError> {
        let is_zip = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
        let (shp, unpacked) = if is_zip {
            let (shp, dir) = unpack(path, temp_dir)?;
            (shp, Some(dir))
        } else {
            (path.to_path_buf(), None)
        };
        let prj = beside(&shp, "prj").ok_or_else(|| ShapefileError::NoProjection {
            path: path.to_path_buf(),
        })?;
        let text = std::fs::read_to_string(&prj).map_err(read_error(&prj))?;
        let crs = prj::crs(&text).map_err(|unsupported| ShapefileError::UnsupportedProjection {
            path: path.to_path_buf(),
            name: match unsupported {
                prj::Unsupported::NotWkt => text.trim().to_owned(),
                prj::Unsupported::Other(name) => name,
            },
        })?;
        let encoding = match beside(&shp, "cpg") {
            Some(cpg) => Some(std::fs::read_to_string(&cpg).map_err(read_error(&cpg))?),
            None => None,
        };
        let (shape_type, records) = record_ranges(path, &shp)?;
        let dbf = beside(&shp, "dbf").unwrap_or_else(|| shp.with_extension("dbf"));
        let (layout, rows) = fields(&dbf, encoding.as_deref())?;
        if rows != records.len() {
            return Err(ShapefileError::RecordCounts {
                path: path.to_path_buf(),
                shapes: records.len(),
                rows,
            });
        }
        info!(
            shapefile.file = %path.display(),
            shapefile.records = records.len(),
            "Opened shapefile"
        );
        Ok(Self(Arc::new(Opened {
            path: path.to_path_buf(),
            shp,
            dbf,
            encoding,
            crs,
            shape_type,
            fields: layout.fields,
            dbf_header: layout.header,
            dbf_record: layout.record,
            records,
            _unpacked: unpacked,
        })))
    }

    /// The fields of the `.dbf` as columns, in its order, each of the kind its type gives.
    #[must_use]
    pub fn columns(&self) -> Vec<ColumnDef> {
        self.0
            .fields
            .iter()
            .map(|field| ColumnDef::Value(field.name.clone(), Some(field.kind())))
            .collect()
    }

    /// The coordinate system of the records.
    #[must_use]
    pub fn crs(&self) -> Crs {
        self.0.crs
    }

    /// The type of every record's geometry, as the header of the `.shp` declares it; `None` for a file of null shapes.
    #[must_use]
    pub fn geometry(&self) -> Option<GeometryType> {
        match self.0.shape_type {
            ShapeType::Point
            | ShapeType::PointM
            | ShapeType::PointZ
            | ShapeType::Multipoint
            | ShapeType::MultipointM
            | ShapeType::MultipointZ => Some(GeometryType::Point),
            ShapeType::Polyline | ShapeType::PolylineM | ShapeType::PolylineZ => Some(GeometryType::Line),
            ShapeType::Polygon | ShapeType::PolygonM | ShapeType::PolygonZ => Some(GeometryType::Polygon),
            ShapeType::NullShape | ShapeType::Multipatch => None,
        }
    }

    /// Whether the `.dbf` has a field named `name`.
    #[must_use]
    pub fn has_field(&self, name: &str) -> bool {
        self.0.fields.iter().any(|field| field.name == name)
    }
}

impl std::fmt::Debug for Shapefile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shapefile")
            .field("path", &self.0.path)
            .field("records", &self.0.records.len())
            .finish_non_exhaustive()
    }
}

/// A function turning an I/O error into a [`ShapefileError`] naming the file at `path`.
fn read_error(path: &Path) -> impl Fn(std::io::Error) -> ShapefileError + '_ {
    |error| ShapefileError::Read {
        path: path.to_path_buf(),
        error,
    }
}

/// The part of the shapefile `shp` with extension `ext`, in lower or upper case, if it exists.
fn beside(shp: &Path, ext: &str) -> Option<PathBuf> {
    [ext.to_owned(), ext.to_ascii_uppercase()]
        .into_iter()
        .map(|ext| shp.with_extension(ext))
        .find(|path| path.is_file())
}

/// Unpacks the parts of the one shapefile of the zip file at `path` into a new directory in `temp_dir`.
fn unpack(path: &Path, temp_dir: &Path) -> Result<(PathBuf, tempfile::TempDir), ShapefileError> {
    let zip_error = |error| ShapefileError::Zip {
        path: path.to_path_buf(),
        error,
    };
    let file = File::open(path).map_err(read_error(path))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).map_err(zip_error)?;
    let names: Vec<String> = archive
        .file_names()
        .map(|name| name.map(std::borrow::Cow::into_owned))
        .collect::<Result<_, _>>()
        .map_err(zip_error)?;
    let shapefiles: Vec<&str> = names
        .iter()
        .filter_map(|name| {
            let (stem, ext) = name.rsplit_once('.')?;
            (ext.eq_ignore_ascii_case("shp") && !name.starts_with("__MACOSX/")).then_some(stem)
        })
        .collect();
    let [stem] = shapefiles[..] else {
        return Err(ShapefileError::ZipShapefiles {
            path: path.to_path_buf(),
            found: shapefiles.len(),
        });
    };
    let dir = tempfile::Builder::new()
        .prefix("martin-shapefile-")
        .tempdir_in(temp_dir)
        .map_err(read_error(temp_dir))?;
    let started = std::time::Instant::now();
    for ext in PARTS {
        let Some(name) = names.iter().find(|name| {
            name.rsplit_once('.')
                .is_some_and(|(s, e)| s == stem && e.eq_ignore_ascii_case(ext))
        }) else {
            continue;
        };
        let target = dir.path().join(format!("shape.{ext}"));
        let mut entry = archive.by_name(name).map_err(zip_error)?;
        let mut out = File::create(&target).map_err(read_error(&target))?;
        std::io::copy(&mut entry, &mut out).map_err(|error| zip_error(error.into()))?;
    }
    info!(
        shapefile.file = %path.display(),
        elapsed = ?started.elapsed(),
        "Unpacked shapefile"
    );
    Ok((dir.path().join("shape.shp"), dir))
}

/// The type of the shapes of the `.shp` at `shp`, and where each record is, from its `.shx`, or from the record
/// headers without one; fails if a record would end past the end of the `.shp`.
fn record_ranges(path: &Path, shp: &Path) -> Result<(ShapeType, Vec<Range<u64>>), ShapefileError> {
    let shapes_error = |error| ShapefileError::Shapes {
        path: path.to_path_buf(),
        error,
    };
    let mut file = BufReader::new(File::open(shp).map_err(read_error(shp))?);
    let header = shapefile::header::Header::read_from(&mut file).map_err(shapes_error)?;
    if header.shape_type == ShapeType::Multipatch {
        return Err(ShapefileError::UnsupportedShapeType {
            path: path.to_path_buf(),
            shape_type: header.shape_type.to_string(),
        });
    }
    let end = u64::try_from(header.file_length).unwrap_or(0) * 2;
    let mut records = Vec::new();
    if let Some(shx) = beside(shp, "shx") {
        let mut index = BufReader::new(File::open(&shx).map_err(read_error(&shx))?);
        let header = shapefile::header::Header::read_from(&mut index).map_err(shapes_error)?;
        let count = (u64::try_from(header.file_length).unwrap_or(0) * 2).saturating_sub(HEADER_BYTES) / 8;
        for _ in 0..count {
            let mut entry = [0u8; 8];
            index.read_exact(&mut entry).map_err(read_error(&shx))?;
            let [o0, o1, o2, o3, l0, l1, l2, l3] = entry;
            let offset = u64::from(u32::from_be_bytes([o0, o1, o2, o3])) * 2;
            let length = u64::from(u32::from_be_bytes([l0, l1, l2, l3])) * 2;
            records.push(offset..offset + RECORD_HEADER_BYTES + length);
        }
    } else {
        let mut offset = HEADER_BYTES;
        while offset + RECORD_HEADER_BYTES <= end {
            let mut record = [0u8; 8];
            file.seek(SeekFrom::Start(offset)).map_err(read_error(shp))?;
            file.read_exact(&mut record).map_err(read_error(shp))?;
            let [_, _, _, _, l0, l1, l2, l3] = record;
            let length = u64::from(u32::from_be_bytes([l0, l1, l2, l3])) * 2;
            records.push(offset..offset + RECORD_HEADER_BYTES + length);
            offset += RECORD_HEADER_BYTES + length;
        }
    }
    let size = file.get_ref().metadata().map_err(read_error(shp))?.len();
    if let Some((record, range)) = (1..).zip(&records).find(|(_, range)| range.end > size) {
        return Err(ShapefileError::RecordPastEnd {
            path: path.to_path_buf(),
            record,
            end: range.end,
            size,
        });
    }
    Ok((header.shape_type, records))
}

/// The fields of the `.dbf` at `dbf` and where they are in its records, with how many records it has.
fn fields(dbf: &Path, encoding: Option<&str>) -> Result<(DbfLayout, usize), ShapefileError> {
    let table = open_dbf(dbf, encoding)?;
    let descriptors = descriptors(dbf)?;
    let fields = table
        .fields()
        .iter()
        .zip(
            descriptors
                .fields
                .into_iter()
                .chain(std::iter::repeat_n((0, 0, 0), usize::MAX)),
        )
        .map(|(field, (decimals, offset, length))| Field {
            name: field.name().to_owned(),
            field_type: field.field_type(),
            decimals: decimals > 0,
            offset,
            length,
        })
        .collect();
    Ok((
        DbfLayout {
            header: descriptors.header,
            record: descriptors.record,
            fields,
        },
        table.num_records(),
    ))
}

/// The fields of a `.dbf` and where its records are.
struct DbfLayout {
    /// Where the first record starts, in bytes.
    header: u64,
    /// How many bytes each record takes.
    record: u64,
    fields: Vec<Field>,
}

/// Opens the `.dbf` at `dbf`, decoding its text as the `.cpg` names, else as its header says.
fn open_dbf(dbf: &Path, encoding: Option<&str>) -> Result<dbase::File<BufReader<File>>, ShapefileError> {
    let error = |error| ShapefileError::Attributes {
        path: dbf.to_path_buf(),
        error,
    };
    let file = BufReader::new(File::open(dbf).map_err(read_error(dbf))?);
    match encoding.and_then(dbase::encoding::DynEncoding::from_name) {
        Some(encoding) => dbase::File::open_with_encoding(file, encoding),
        None => dbase::File::open(file),
    }
    .map_err(error)
}

/// The field descriptors of the `.dbf` at `dbf`, which `dbase` does not expose.
struct Descriptors {
    /// Where the first record starts, in bytes.
    header: u64,
    /// How many bytes each record takes.
    record: u64,
    /// The decimal places, offset in a record and length in bytes of each field.
    fields: Vec<(u8, u64, usize)>,
}

/// Reads the field descriptors of the `.dbf` at `dbf`.
fn descriptors(dbf: &Path) -> Result<Descriptors, ShapefileError> {
    let mut file = BufReader::new(File::open(dbf).map_err(read_error(dbf))?);
    let mut header = [0u8; 32];
    file.read_exact(&mut header).map_err(read_error(dbf))?;
    let mut fields = Vec::new();
    let mut offset = 1;
    loop {
        let mut descriptor = [0u8; 32];
        file.read_exact(&mut descriptor[..1]).map_err(read_error(dbf))?;
        if descriptor[0] == 0x0d {
            return Ok(Descriptors {
                header: u64::from(u16::from_le_bytes([header[8], header[9]])),
                record: u64::from(u16::from_le_bytes([header[10], header[11]])),
                fields,
            });
        }
        file.read_exact(&mut descriptor[1..]).map_err(read_error(dbf))?;
        fields.push((descriptor[17], offset, usize::from(descriptor[16])));
        offset += u64::from(descriptor[16]);
    }
}

/// The integer a numeric field holds as text in record `record` of the `.dbf` `dbf`, read from `raw` as `dbase`
/// reads it only as a float; `None` if the text is not one.
fn exact_integer(raw: &mut File, opened: &Opened, record: usize, field: &Field) -> Result<Option<i64>, ShapefileError> {
    let at = opened.dbf_header + record as u64 * opened.dbf_record + field.offset;
    let mut text = vec![0u8; field.length];
    raw.seek(SeekFrom::Start(at))
        .and_then(|_| raw.read_exact(&mut text))
        .map_err(read_error(&opened.dbf))?;
    Ok(std::str::from_utf8(&text)
        .ok()
        .and_then(|text| text.trim().parse().ok()))
}

/// Which records of its file a table reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Records {
    /// Every record.
    #[default]
    All,
    /// Only the record with this number, counting from 1, which is also its feature id.
    One(u64),
}

/// One table of the plan, read from a shapefile.
#[derive(Clone, Debug)]
pub struct ShapefileTable {
    /// The shapefile.
    pub file: Shapefile,
    /// Only records touching these WGS84 bounds, if set.
    pub bounds: Option<Bounds>,
    /// Which records are read.
    pub records: Records,
}

/// Shapefiles, one per table of the plan, each split into partitions of whole records.
pub struct ShapefileSource {
    plan: Plan,
    scans: Vec<Scan>,
    /// Each partition's scan and the records it reads, by position.
    partitions: Vec<(u16, Range<usize>)>,
}

/// One table's shapefile, and what to read from it.
struct Scan {
    file: Shapefile,
    /// The `.dbf` field of each scanned column, by position.
    fields: Vec<usize>,
    /// Only records touching this box, in the file's coordinates, if set.
    bounds: Option<Rect>,
}

impl ShapefileSource {
    /// Plans `tables` and splits each table's records into partitions.
    ///
    /// # Errors
    /// On an invalid plan.
    pub fn new(tables: Vec<(TableDef, ShapefileTable)>) -> Result<Self, TileGenError> {
        let (defs, files): (Vec<_>, Vec<_>) = tables.into_iter().unzip();
        let plan = Plan::new(defs)?;
        let mut scans = Vec::with_capacity(files.len());
        let mut partitions = Vec::new();
        for (index, table) in files.into_iter().enumerate() {
            let opened = &table.file.0;
            let fields = plan
                .columns(index)
                .iter()
                .map(|column| {
                    opened
                        .fields
                        .iter()
                        .position(|field| field.name == *column)
                        .expect("the plan scans only columns of the file")
                })
                .collect();
            let index = u16::try_from(index).expect("the plan has fewer than 2^16 tables");
            match table.records {
                Records::All => {
                    let mut start = 0;
                    while start < opened.records.len() {
                        let first = opened.records[start].start;
                        let end = (start + 1..opened.records.len())
                            .find(|&i| opened.records[i].end.saturating_sub(first) > PARTITION_BYTES)
                            .unwrap_or(opened.records.len());
                        partitions.push((index, start..end));
                        start = end;
                    }
                }
                Records::One(number) => {
                    if let Some(record) = usize::try_from(number)
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .filter(|&i| i < opened.records.len())
                    {
                        partitions.push((index, record..record + 1));
                    }
                }
            }
            scans.push(Scan {
                bounds: table.bounds.map(|bounds| source_rect(bounds, opened.crs)),
                file: table.file,
                fields,
            });
        }
        Ok(Self {
            plan,
            scans,
            partitions,
        })
    }

    /// The plan the source reads its tables for.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

/// The WGS84 `bounds` in the coordinates `crs`.
fn source_rect(bounds: Bounds, crs: Crs) -> Rect {
    match crs {
        Crs::Wgs84 => Rect::new((bounds.left, bounds.bottom), (bounds.right, bounds.top)),
        Crs::WebMercator => {
            let clamp = |lat: f64| lat.clamp(-MAX_LATITUDE, MAX_LATITUDE);
            let min = wgs84_to_webmercator(bounds.left, clamp(bounds.bottom));
            let max = wgs84_to_webmercator(bounds.right, clamp(bounds.top));
            Rect::new(min, max)
        }
    }
}

impl FeatureSource for ShapefileSource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.partitions.len()).expect("fewer than 2^32 partitions")
    }

    fn estimated_features(&self) -> Option<u64> {
        Some(self.partitions.iter().map(|(_, records)| records.len() as u64).sum())
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        ShapefileReader::new(self).read(partition, keys, emit)
    }

    fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
        Ok((0..count.max(1))
            .map(|_| Box::new(ShapefileReader::new(self)) as Box<dyn FeatureReader + '_>)
            .collect())
    }
}

impl std::fmt::Debug for ShapefileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapefileSource")
            .field("partitions", &self.partitions.len())
            .finish_non_exhaustive()
    }
}

/// One reader thread: the open files of each scan it has read, kept from partition to partition.
struct ShapefileReader<'a> {
    source: &'a ShapefileSource,
    files: Vec<Option<OpenFiles>>,
}

/// The `.shp` and `.dbf` of a scan, open for reading.
struct OpenFiles {
    shp: File,
    dbf: dbase::File<BufReader<File>>,
    /// The `.dbf` again, for the text of integers too large for the float `dbase` reads them as.
    raw_dbf: File,
}

impl<'a> ShapefileReader<'a> {
    /// A reader that opens each scan's files once it reads from them.
    fn new(source: &'a ShapefileSource) -> Self {
        Self {
            source,
            files: source.scans.iter().map(|_| None).collect(),
        }
    }
}

impl FeatureReader for ShapefileReader<'_> {
    fn read(
        &mut self,
        partition: u32,
        _keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let (index, records) = self
            .source
            .partitions
            .get(partition as usize)
            .ok_or(SourceError::UnknownPartition(partition))?;
        let scan = &self.source.scans[usize::from(*index)];
        let opened = &scan.file.0;
        let files = match &mut self.files[usize::from(*index)] {
            Some(files) => files,
            empty => empty.insert(OpenFiles {
                shp: File::open(&opened.shp).map_err(read_error(&opened.shp))?,
                dbf: open_dbf(&opened.dbf, opened.encoding.as_deref())?,
                raw_dbf: File::open(&opened.dbf).map_err(read_error(&opened.dbf))?,
            }),
        };
        let ranges = &opened.records[records.clone()];
        let first = ranges.iter().map(|r| r.start).min().unwrap_or(0);
        let last = ranges.iter().map(|r| r.end).max().unwrap_or(first);
        let mut bytes = vec![0u8; usize::try_from(last - first).expect("a partition fits in memory")];
        files
            .shp
            .seek(SeekFrom::Start(first))
            .and_then(|_| files.shp.read_exact(&mut bytes))
            .map_err(read_error(&opened.shp))?;
        let mut shapes = Cursor::new(bytes);
        let mut batch = Vec::with_capacity(BATCH_FEATURES.min(ranges.len()));
        let mut first_row = 0;
        for (record, range) in records.clone().zip(ranges) {
            let number = record as u64 + 1;
            let shape = read_shape(&mut shapes, range.start - first, range.end - range.start).map_err(|error| {
                ShapefileError::Record {
                    path: opened.path.clone(),
                    record: number,
                    error,
                }
            })?;
            let Some(geometry) = geometry(shape) else {
                continue;
            };
            if let Some(bounds) = scan.bounds
                && !geometry_rect(&geometry).is_some_and(|rect| rect.intersects(&bounds))
            {
                continue;
            }
            let attributes_error = |error| ShapefileError::Attributes {
                path: opened.dbf.clone(),
                error,
            };
            let mut row = files.dbf.record(record).ok_or_else(|| ShapefileError::RecordCounts {
                path: opened.path.clone(),
                shapes: opened.records.len(),
                rows: record,
            })?;
            if row.is_deleted().map_err(attributes_error)? {
                continue;
            }
            let mut props = Vec::with_capacity(scan.fields.len());
            for (pos, &field) in (0u32..).zip(&scan.fields) {
                let value = row.read_field(FieldIndex(field)).map_err(attributes_error)?;
                let info = &opened.fields[field];
                let exact = if let FieldValue::Numeric(Some(number)) = value
                    && info.kind() == ValueKind::Int
                    && number.abs() >= MAX_EXACT
                {
                    exact_integer(&mut files.raw_dbf, opened, record, info)?.map(Prop::I64)
                } else {
                    None
                };
                if let Some(prop) = exact.or_else(|| prop(value, info.kind())) {
                    props.push((KeyId::new(pos), prop));
                }
            }
            batch.push(SourceFeature {
                id: i64::try_from(number).ok(),
                geometry,
                props,
                relations: Vec::new(),
            });
            if batch.len() >= BATCH_FEATURES {
                let features = std::mem::replace(&mut batch, Vec::with_capacity(BATCH_FEATURES));
                emit_batch(emit, *index, partition, &mut first_row, opened.crs, features)?;
            }
        }
        if batch.is_empty() {
            return Ok(());
        }
        emit_batch(emit, *index, partition, &mut first_row, opened.crs, batch)
    }
}

/// Sends `features` on as the next batch of the partition, advancing `first_row` past them.
fn emit_batch(
    emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    table: u16,
    partition: u32,
    first_row: &mut u64,
    crs: Crs,
    features: Vec<SourceFeature>,
) -> SourceResult<()> {
    let row = *first_row;
    *first_row += features.len() as u64;
    emit(FeatureBatch {
        table,
        partition,
        first_row: row,
        crs,
        features,
    })
}

/// Reads the shape of the record at `offset` of `shapes`, `length` bytes with its header.
fn read_shape(shapes: &mut Cursor<Vec<u8>>, offset: u64, length: u64) -> Result<Shape, shapefile::Error> {
    shapes.set_position(offset + RECORD_HEADER_BYTES);
    let content = i32::try_from(length.saturating_sub(RECORD_HEADER_BYTES))
        .map_err(|_too_long| shapefile::Error::InvalidShapeRecordSize)?;
    Shape::read_from(shapes, content)
}

/// The value of a `.dbf` field as a property of `kind`; `None` for an empty value.
fn prop(value: FieldValue, kind: ValueKind) -> Option<Prop<SourceStr>> {
    Some(match value {
        FieldValue::Character(text) => Prop::Str(SourceStr::from(text?)),
        FieldValue::Memo(text) => Prop::Str(SourceStr::from((!text.is_empty()).then_some(text)?)),
        FieldValue::Numeric(number) => {
            let number = number?;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a field without decimal places holds integers"
            )]
            if kind == ValueKind::Int && number.fract() == 0.0 {
                Prop::I64(number as i64)
            } else {
                Prop::F64(number)
            }
        }
        FieldValue::Float(number) => Prop::F64(f64::from(number?)),
        FieldValue::Integer(number) => Prop::I64(i64::from(number)),
        FieldValue::Currency(number) | FieldValue::Double(number) => Prop::F64(number),
        FieldValue::Logical(value) => Prop::Bool(value?),
        FieldValue::Date(date) => {
            let date = date?;
            Prop::Str(SourceStr::from(format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day()
            )))
        }
        FieldValue::DateTime(at) => {
            let (date, time) = (at.date(), at.time());
            Prop::Str(SourceStr::from(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                date.year(),
                date.month(),
                date.day(),
                time.hours(),
                time.minutes(),
                time.seconds()
            )))
        }
    })
}

/// The coordinates of `points` as a line.
fn line<P: HasXY>(points: &[P]) -> LineString {
    points.iter().map(|p| Coord { x: p.x(), y: p.y() }).collect()
}

/// The geometry of a shape; `None` for a null shape or one without coordinates.
fn geometry(shape: Shape) -> Option<Geometry> {
    let geometry = match shape {
        Shape::NullShape | Shape::Multipatch(_) => return None,
        Shape::Point(p) => Geometry::Point(Coord { x: p.x, y: p.y }),
        Shape::PointM(p) => Geometry::Point(Coord { x: p.x, y: p.y }),
        Shape::PointZ(p) => Geometry::Point(Coord { x: p.x, y: p.y }),
        Shape::Multipoint(points) => Geometry::Points(line(points.points()).0),
        Shape::MultipointM(points) => Geometry::Points(line(points.points()).0),
        Shape::MultipointZ(points) => Geometry::Points(line(points.points()).0),
        Shape::Polyline(lines) => lines_of(lines.parts()),
        Shape::PolylineM(lines) => lines_of(lines.parts()),
        Shape::PolylineZ(lines) => lines_of(lines.parts()),
        Shape::Polygon(polygon) => polygons_of(polygon.rings()),
        Shape::PolygonM(polygon) => polygons_of(polygon.rings()),
        Shape::PolygonZ(polygon) => polygons_of(polygon.rings()),
    };
    let empty = match &geometry {
        Geometry::Point(p) => p.x.is_nan() || p.y.is_nan(),
        Geometry::Points(points) => points.is_empty(),
        Geometry::Lines(lines) => lines.is_empty(),
        Geometry::Polygons(polygons) => polygons.is_empty(),
        Geometry::Line(_) | Geometry::Outline(_) | Geometry::Polygon(_) | Geometry::Wkb(_) => false,
    };
    (!empty).then_some(geometry)
}

/// The parts of a polyline, as one line if it has one part.
fn lines_of<P: HasXY>(parts: &[Vec<P>]) -> Geometry {
    let mut lines: Vec<LineString> = parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(|part| line(part))
        .collect();
    if lines.len() == 1 {
        Geometry::Line(lines.pop().expect("one line"))
    } else {
        Geometry::Lines(lines)
    }
}

/// The rings of a shapefile polygon as polygons, as `GeoTools` reads them: each clockwise ring an outer ring, and each
/// counter-clockwise ring a hole of the only outer ring, else of the smallest one around it, or its own polygon.
fn polygons_of<P: HasXY>(rings: &[PolygonRing<P>]) -> Geometry {
    let mut shells = Vec::new();
    let mut holes = Vec::new();
    for ring in rings {
        match ring {
            PolygonRing::Outer(points) if !points.is_empty() => shells.push(line(points)),
            PolygonRing::Inner(points) if !points.is_empty() => holes.push(line(points)),
            PolygonRing::Outer(_) | PolygonRing::Inner(_) => {}
        }
    }
    let mut polygons: Vec<Polygon> = shells
        .into_iter()
        .map(|shell| Polygon::new(shell, Vec::new()))
        .collect();
    for hole in holes {
        let Some(&inside) = hole.0.first() else {
            continue;
        };
        let around = if polygons.len() == 1 {
            Some(0)
        } else {
            polygons
                .iter()
                .enumerate()
                .filter(|(_, polygon)| polygon.contains(&inside))
                .min_by(|(_, a), (_, b)| ring_area(a.exterior()).total_cmp(&ring_area(b.exterior())))
                .map(|(i, _)| i)
        };
        match around {
            Some(i) => polygons[i].interiors_push(hole),
            None => polygons.push(Polygon::new(hole, Vec::new())),
        }
    }
    if polygons.len() == 1 {
        Geometry::Polygon(polygons.pop().expect("one polygon"))
    } else {
        Geometry::Polygons(polygons)
    }
}

/// The area enclosed by the closed `ring`, whatever its orientation.
fn ring_area(ring: &LineString) -> f64 {
    let twice: f64 = ring.0.windows(2).map(|w| w[0].x * w[1].y - w[1].x * w[0].y).sum();
    twice.abs() / 2.0
}

/// The bounding box of `geometry`, if it has coordinates.
fn geometry_rect(geometry: &Geometry) -> Option<Rect> {
    use geo::BoundingRect as _;
    let union = |a: Rect, b: Rect| {
        Rect::new(
            (a.min().x.min(b.min().x), a.min().y.min(b.min().y)),
            (a.max().x.max(b.max().x), a.max().y.max(b.max().y)),
        )
    };
    match geometry {
        Geometry::Point(p) => Some(Rect::new(*p, *p)),
        Geometry::Line(line) | Geometry::Outline(line) => line.bounding_rect(),
        Geometry::Polygon(polygon) => polygon.bounding_rect(),
        Geometry::Points(points) => points.iter().map(|p| Rect::new(*p, *p)).reduce(union),
        Geometry::Lines(lines) => lines.iter().filter_map(LineString::bounding_rect).reduce(union),
        Geometry::Polygons(polygons) => polygons.iter().filter_map(Polygon::bounding_rect).reduce(union),
        Geometry::Wkb(_) => None,
    }
}
