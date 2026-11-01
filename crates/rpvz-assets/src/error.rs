use std::path::PathBuf;

/// Everything that can go wrong while finding, fetching or installing game data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid archive {path}: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("invalid PopCap pak archive: {0}")]
    Pak(String),

    #[error("invalid VDF file {path}: {reason}")]
    Vdf { path: PathBuf, reason: String },

    #[error("network error: {0}")]
    Http(String),

    #[error("{what} not found: {detail}")]
    NotFound { what: &'static str, detail: String },

    #[error("hash mismatch for {what}: expected {expected}, got {actual}")]
    HashMismatch {
        what: String,
        expected: String,
        actual: String,
    },

    #[error("unsafe path in archive: {0}")]
    UnsafePath(String),

    #[error("invalid manifest {path}: {reason}")]
    Manifest { path: PathBuf, reason: String },

    #[error("no data directory could be determined for this platform")]
    NoDataDir,

    #[error("game data format: {0}")]
    Format(#[from] rpvz_formats::FormatError),

    #[error("image conversion: {0}")]
    Image(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Attaches a path to raw I/O errors.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.into(),
            source,
        })
    }
}

impl<T> IoContext<T> for zip::result::ZipResult<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Zip {
            path: path.into(),
            source,
        })
    }
}
