//! Close-period state carried by year-file metadata.

use crate::CloseError;

/// Close state for one fiscal year.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosingState {
    /// The year accepts ordinary ledger mutations.
    Open,
    /// The year is under close review.
    Review,
    /// The year is closed and ordinary mutations are rejected.
    Closed,
}

impl ClosingState {
    /// Returns the metadata spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Review => "review",
            Self::Closed => "closed",
        }
    }

    /// Parses a metadata spelling.
    pub fn parse(value: &str) -> Result<Self, CloseError> {
        match value {
            "open" => Ok(Self::Open),
            "review" => Ok(Self::Review),
            "closed" => Ok(Self::Closed),
            other => Err(CloseError::InvalidState(format!(
                "unknown closing state {other}"
            ))),
        }
    }
}
