use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    #[error("Walk directory error {0}: {1}")]
    DirectoryWalking(walkdir::Error, PathBuf),
}
