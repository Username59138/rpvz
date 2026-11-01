#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FormatError {
    #[error("{what}: data ends at {len} bytes, needed {needed} at offset {offset}")]
    Truncated {
        what: &'static str,
        offset: usize,
        needed: usize,
        len: usize,
    },

    #[error("{what}: bad magic {found:02x?}")]
    BadMagic { what: &'static str, found: Vec<u8> },

    #[error("unsupported {0}")]
    Unsupported(String),

    #[error("invalid {0}")]
    Invalid(String),

    #[error("zlib: {0}")]
    Zlib(String),
}

pub type Result<T, E = FormatError> = std::result::Result<T, E>;
