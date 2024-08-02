/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::BitOr;

use anyhow::anyhow;
use anyhow::Result;
use hgstore::parse_copy_from_hg_file_metadata;
use minibytes::Bytes;
use types::Key;

use crate::scmstore::file::LazyFile;
use crate::scmstore::value::StoreValue;
use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;

#[derive(Debug, Default)]
pub struct StoreFile {
    // TODO(meyer): We'll probably eventually need a better "canonical lazy file" abstraction, since SaplingRemoteApi FileEntry won't always carry content
    pub(crate) content: Option<LazyFile>,
    pub(crate) aux_data: Option<FileAuxData>,
}

impl StoreValue for StoreFile {
    type Attrs = FileAttributes;

    /// Returns which attributes are present in this StoreFile
    fn attrs(&self) -> FileAttributes {
        FileAttributes {
            pure_content: self.content.is_some(),
            content_header: self
                .content
                .as_ref()
                // All content sources have hg file header except CAS.
                .is_some_and(|f| !matches!(f, LazyFile::Cas(_)))
                // File header can also come from AUX data.
                || self
                    .aux_data
                    .as_ref()
                    .is_some_and(|aux| aux.file_header_metadata.is_some()),
            aux_data: self.aux_data.is_some(),
        }
    }

    /// Return a StoreFile with only the specified subset of attributes
    fn mask(self, attrs: FileAttributes) -> Self {
        StoreFile {
            content: if attrs.pure_content || attrs.content_header {
                self.content
            } else {
                None
            },
            aux_data: if attrs.aux_data { self.aux_data } else { None },
        }
    }
}

impl StoreFile {
    pub fn aux_data(&self) -> Result<FileAuxData> {
        self.aux_data
            .clone()
            .ok_or_else(|| anyhow!("no aux data available"))
    }

    pub(crate) fn compute_aux_data(&mut self) -> Result<()> {
        self.aux_data = Some(
            self.content
                .as_mut()
                .ok_or_else(|| anyhow!("failed to compute aux data, no content available"))?
                .aux_data()?,
        );
        Ok(())
    }

    pub fn file_content(&mut self) -> Result<Bytes> {
        self.content
            .as_mut()
            .ok_or_else(|| anyhow!("no content available"))?
            .file_content()
    }

    pub fn file_content_with_copy_info(&mut self) -> Result<(Bytes, Option<Key>)> {
        let content = self
            .content
            .as_mut()
            .ok_or_else(|| anyhow!("no content available"))?;

        // Prefer getting content header info from aux data since that is more compatible
        // with CAS (which won't contain header).
        if let Some(FileAuxData {
            file_header_metadata: Some(header),
            ..
        }) = &self.aux_data
        {
            Ok((
                content.file_content()?,
                parse_copy_from_hg_file_metadata(header)?,
            ))
        } else {
            content.file_content_with_copy_info()
        }
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
