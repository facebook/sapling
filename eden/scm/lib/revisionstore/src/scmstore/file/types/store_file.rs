/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::BitOr;

use anyhow::anyhow;
use anyhow::Result;
use minibytes::Bytes;
use tracing::instrument;

use crate::scmstore::file::LazyFile;
use crate::scmstore::value::StoreValue;
use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;

#[derive(Debug)]
pub struct StoreFile {
    // TODO(meyer): We'll probably eventually need a better "canonical lazy file" abstraction, since EdenApi FileEntry won't always carry content
    pub(crate) content: Option<LazyFile>,
    pub(crate) aux_data: Option<FileAuxData>,
}

impl StoreValue for StoreFile {
    type Attrs = FileAttributes;

    /// Returns which attributes are present in this StoreFile
    fn attrs(&self) -> FileAttributes {
        FileAttributes {
            content: self.content.is_some(),
            aux_data: self.aux_data.is_some(),
        }
    }

    /// Return a StoreFile with only the specified subset of attributes
    fn mask(self, attrs: FileAttributes) -> Self {
        StoreFile {
            content: if attrs.content { self.content } else { None },
            aux_data: if attrs.aux_data { self.aux_data } else { None },
        }
    }
}

impl StoreFile {
    pub fn aux_data(&self) -> Result<FileAuxData> {
        self.aux_data
            .ok_or_else(|| anyhow!("no aux data available"))
    }

    #[instrument(level = "debug", skip(self))]
    pub(crate) fn compute_aux_data(&mut self) -> Result<()> {
        self.aux_data = Some(
            self.content
                .as_mut()
                .ok_or_else(|| anyhow!("failed to compute aux data, no content available"))?
                .aux_data()?,
        );
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn file_content(&mut self) -> Result<Bytes> {
        self.content
            .as_mut()
            .ok_or_else(|| anyhow!("no content available"))?
            .file_content()
    }
}

impl BitOr for StoreFile {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        StoreFile {
            content: self.content.or(rhs.content),
            aux_data: self.aux_data.or(rhs.aux_data),
        }
    }
}

impl Default for StoreFile {
    fn default() -> Self {
        StoreFile {
            content: None,
            aux_data: None,
        }
    }
}

impl From<FileAuxData> for StoreFile {
    fn from(v: FileAuxData) -> Self {
        StoreFile {
            content: None,
            aux_data: Some(v),
        }
    }
}

impl From<LazyFile> for StoreFile {
    fn from(v: LazyFile) -> Self {
        StoreFile {
            content: Some(v),
            aux_data: None,
        }
    }
}
