/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::BitOr;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use format_util::parse_copy_from_hg_file_metadata;
use minibytes::Bytes;
use types::Key;

use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;
use crate::scmstore::file::LazyFile;
use crate::scmstore::value::StoreValue;

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
            content_header: self.content_contains_hg_header() || self.aux_data_contains_hg_header(),
            aux_data: self.aux_data.is_some(),
        }
    }

    /// Return a StoreFile with only the specified subset of attributes
    fn mask(self, attrs: FileAttributes) -> Self {
        // hg content header can come from either content or aux data. Be sure not to mask
        // off the content header if it was requested.
        let content_has_header = self.content_contains_hg_header();
        let aux_has_header = self.aux_data_contains_hg_header();
        StoreFile {
            content: if attrs.pure_content || (attrs.content_header && content_has_header) {
                self.content
            } else {
                None
            },
            aux_data: if attrs.aux_data || (attrs.content_header && aux_has_header) {
                self.aux_data
            } else {
                None
            },
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
                .as_ref()
                .ok_or_else(|| anyhow!("failed to compute aux data, no content available"))?
                .aux_data()?,
        );
        Ok(())
    }

    // Pure file content without hg copy info.
    pub fn file_content(&self) -> Result<Blob> {
        self.content
            .as_ref()
            .ok_or_else(|| anyhow!("no content available"))?
            .file_content()
            .map(|(content, _header)| content)
    }

    // File content including hg copy info header.
    pub fn hg_content(&self) -> Result<Bytes> {
        if let Some(LazyFile::Cas(ref pure_content)) = self.content {
            if let Some(FileAuxData {
                file_header_metadata: Some(ref header),
                ..
            }) = self.aux_data
            {
                // another implicit copy of underlying content of IOBuf here
                let pure_content = pure_content.to_bytes();
                if header.is_empty() {
                    Ok(pure_content)
                } else {
                    Ok([header.as_ref(), pure_content.as_ref()].concat().into())
                }
            } else {
                bail!("CAS file content with no hg content header in aux data");
            }
        } else {
            self.content
                .as_ref()
                .ok_or_else(|| anyhow!("no content available"))?
                .hg_content()
        }
    }

    pub fn copy_info(&self) -> Result<Option<Key>> {
        if let Some(FileAuxData {
            file_header_metadata: Some(ref header),
            ..
        }) = self.aux_data
        {
            return parse_copy_from_hg_file_metadata(header.as_ref());
        }

        if let Some(content) = &self.content {
            if let (_, Some(header)) = content.file_content()? {
                return parse_copy_from_hg_file_metadata(header.as_ref());
            }
        }

        Ok(None)
    }

    fn content_contains_hg_header(&self) -> bool {
        self.content
            .as_ref()
            .is_some_and(|f| !matches!(f, LazyFile::Cas(_)))
    }

    fn aux_data_contains_hg_header(&self) -> bool {
        matches!(
            self.aux_data.as_ref(),
            Some(FileAuxData {
                file_header_metadata: Some(_),
                ..
            })
        )
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
