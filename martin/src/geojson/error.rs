use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum GeoJsonError {
    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),

    #[error("File {1} is not a valid GeoJSON: {0}")]
    NotValidGeoJson(serde_json::Error, PathBuf),

    #[error("GeoJSON File {0} has no geometry which could be served as tiles")]
    NoGeometry(PathBuf),
}
