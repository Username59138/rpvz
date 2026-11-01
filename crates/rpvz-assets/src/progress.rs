//! Progress events, so the CLI and the game's loading screen can both show them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Download,
    Verify,
    Extract,
    Finalize,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Download => "Downloading",
            Stage::Verify => "Verifying",
            Stage::Extract => "Extracting",
            Stage::Finalize => "Finishing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// A new stage has started.
    Stage(Stage),
    /// Byte-level progress of the current stage. `total` is unknown when the
    /// server sends no length.
    Bytes { done: u64, total: Option<u64> },
    /// File-level progress of the current stage.
    Files { done: usize, total: usize },
}

impl Progress {
    /// Fraction in `0.0..=1.0`, when it can be computed.
    pub fn fraction(&self) -> Option<f32> {
        match *self {
            Progress::Bytes {
                done,
                total: Some(total),
            } if total > 0 => Some((done as f64 / total as f64).min(1.0) as f32),
            Progress::Files { done, total } if total > 0 => {
                Some((done as f32 / total as f32).min(1.0))
            }
            _ => None,
        }
    }
}

/// Callback type used throughout the crate.
pub type ProgressFn<'a> = dyn FnMut(Progress) + 'a;
