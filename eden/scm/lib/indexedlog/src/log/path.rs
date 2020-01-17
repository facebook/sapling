/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::IoResultExt;
use crate::lock::ScopedDirLock;
use crate::log::{LogMetadata, META_FILE};
use crate::utils;
use std::path::PathBuf;

/// Abstract Path for [`Log`].
///
/// This defines where a [`Log`] reads and writes data.
#[derive(Clone, Debug)]
pub enum GenericPath {
    /// The [`Log`] is backed by a directory on filesystem.
    Filesystem(PathBuf),

    /// From nothing. Indicates creating from memory.
    Nothing,
}

impl From<&std::path::Path> for GenericPath {
    fn from(path: &std::path::Path) -> Self {
        Self::Filesystem(path.to_path_buf())
    }
}

impl From<&str> for GenericPath {
    fn from(path: &str) -> Self {
        Self::Filesystem(std::path::Path::new(path).to_path_buf())
    }
}

impl From<PathBuf> for GenericPath {
    fn from(path: PathBuf) -> Self {
        Self::Filesystem(path)
    }
}

impl From<&PathBuf> for GenericPath {
    fn from(path: &PathBuf) -> Self {
        Self::Filesystem(path.clone())
    }
}

impl From<()> for GenericPath {
    fn from(_path: ()) -> Self {
        Self::Nothing
    }
}

impl GenericPath {
    /// Return the main filesystem path.
    pub fn as_opt_path(&self) -> Option<&std::path::Path> {
        match self {
            GenericPath::Filesystem(path) => Some(&path),
            GenericPath::Nothing => None,
        }
    }

    pub(crate) fn mkdir(&self) -> crate::Result<()> {
        if let Some(dir) = self.as_opt_path() {
            utils::mkdir_p(dir)
        } else {
            Ok(())
        }
    }

    pub(crate) fn lock(&self) -> crate::Result<ScopedDirLock> {
        if let Some(dir) = self.as_opt_path() {
            Ok(ScopedDirLock::new(&dir)?)
        } else {
            Err(crate::Error::programming(
                "read_meta() does not support GenericPath::Nothing",
            ))
        }
    }

    pub(crate) fn read_meta(&self) -> crate::Result<LogMetadata> {
        match self {
            GenericPath::Filesystem(dir) => {
                let meta_path = dir.join(META_FILE);
                LogMetadata::read_file(&meta_path).context(&meta_path, "when reading LogMetadata")
            }
            GenericPath::Nothing => Err(crate::Error::programming(
                "read_meta() does not support GenericPath::Nothing",
            )),
        }
    }

    pub(crate) fn write_meta(&self, meta: &LogMetadata, fsync: bool) -> crate::Result<()> {
        match self {
            GenericPath::Filesystem(dir) => {
                let meta_path = dir.join(META_FILE);
                meta.write_file(&meta_path, fsync)?;
                Ok(())
            }
            GenericPath::Nothing => Err(crate::Error::programming(
                "write_meta() does not support GenericPath::Nothing",
            )),
        }
    }
}
