//! Errors specific to keymaps.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors specific to keymaps.
#[derive(Debug, Error)]
pub enum KeymapError {
    /// Error when encountering an unknown modifier.
    #[error("unknown modifier: {0}")]
    UnknownModifier(String),

    /// Error when a key definition is missing.
    #[error("key definition missing")]
    MissingDefinition,

    /// Error when a keymap is missing.
    #[error("keymap not found: {0}")]
    MissingKeymap(String),

    /// Error when a key is unrecognised.
    #[error("unrecognised key: {0}")]
    UnknownKey(String),

    /// Parsing error.
    #[cfg(feature = "keymap-file")]
    #[error("parse error: {0}")]
    Parse(#[from] pest::error::Error<crate::keymap_file::Rule>),

    /// Error related to parsing a binding within a keymap.
    #[error("keybinding error")]
    Binding(#[from] crate::bindings::BindingError),

    /// Wrapped error within the context of a file.
    #[error("error loading file '{file}'")]
    WithFile {
        /// Wrapped error.
        #[source]
        error: Box<KeymapError>,

        /// File the error is about.
        file: PathBuf,
    },
}

impl KeymapError {
    #[allow(unused)]
    pub(crate) fn with_file(self, file: impl AsRef<Path>) -> Self {
        Self::WithFile {
            error: Box::new(self),
            file: file.as_ref().to_owned(),
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, KeymapError>;
