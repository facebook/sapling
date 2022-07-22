/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::lock::ScopedDirLock;
use crate::log::LogMetadata;
use crate::log::META_FILE;
use crate::utils;

/// Abstract Path for [`Log`].
///
/// This defines where a [`Log`] reads and writes data.
#[derive(Clone, Debug)]
pub enum GenericPath {
    /// The [`Log`] is backed by a directory on filesystem.
    Filesystem(PathBuf),

    /// Metadata is shared (between `Log` and `MultiLog`).
    /// Other parts still use `path`.
    SharedMeta {
        path: Box<GenericPath>,
        meta: Arc<Mutex<LogMetadata>>,
    },

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
            GenericPath::SharedMeta { path, .. } => path.as_opt_path(),
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
                LogMetadata::read_file(&meta_path)
            }
            GenericPath::SharedMeta { meta, path } => {
                let meta = meta.lock().unwrap();
                if let GenericPath::Filesystem(dir) = path.as_ref() {
                    let meta_path = dir.join(META_FILE);
                    if let Ok(on_disk_meta) = LogMetadata::read_file(&meta_path) {
                        // Prefer the per-log "meta" if it is compatible with the multi-meta.
                        // The per-log meta might contain more up-to-date information about
                        // indexes, etc.
                        if meta.is_compatible_with(&on_disk_meta) {
                            return Ok(on_disk_meta);
                        }
                    }
                }
                Ok(meta.clone())
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
            GenericPath::SharedMeta {
                meta: shared_meta,
                path,
            } => {
                // Update the per-log "meta" file. This can be useful for
                // picking up new indexes (see test_new_index_built_only_once),
                // or log internal data investigation.
                if let GenericPath::Filesystem(dir) = path.as_ref() {
                    let meta_path = dir.join(META_FILE);
                    meta.write_file(&meta_path, fsync)?;
                }
                let mut shared_meta = shared_meta.lock().unwrap();
                *shared_meta = meta.clone();
                Ok(())
            }
            GenericPath::Nothing => Err(crate::Error::programming(
                "write_meta() does not support GenericPath::Nothing",
            )),
        }
    }
}
