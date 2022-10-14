/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use bytes::Bytes;
use futures::try_join;
pub use xdiff::CopyInfo;

use crate::changeset_path::ChangesetPathContentContext;
use crate::errors::MononokeError;
use crate::file::FileType;

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between two corresponding
/// files in the commits.
///
/// The changed, copied and moved variants contain the items in the same
/// order as the commits that were compared, i.e. in `a.diff(b)`, they
/// will contain `(a, b)`.  This usually means the destination is first.
#[derive(Clone, Debug)]
pub enum ChangesetPathDiffContext {
    Added(ChangesetPathContentContext),
    Removed(ChangesetPathContentContext),
    Changed(ChangesetPathContentContext, ChangesetPathContentContext),
    Copied(ChangesetPathContentContext, ChangesetPathContentContext),
    Moved(ChangesetPathContentContext, ChangesetPathContentContext),
}

/// A diff between two files in extended unified diff format
pub struct UnifiedDiff {
    /// Raw diff as bytes.
    pub raw_diff: Vec<u8>,
    /// One of the diffed files is binary, raw diff contains just a placeholder.
    pub is_binary: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UnifiedDiffMode {
    /// Unified diff is generated inline as normal.
    Inline,
    /// Content is not fetched - instead a placeholder diff like
    ///
    /// diff --git a/file.txt b/file.txt
    /// Binary file file.txt has changed
    ///
    /// is generated
    OmitContent,
}

impl ChangesetPathDiffContext {
    /// Create a new path diff context that compares the contents of two
    /// changeset paths.
    ///
    /// Copy information must be provided if the file has been copied or
    /// moved.
    pub fn new(
        base: Option<ChangesetPathContentContext>,
        other: Option<ChangesetPathContentContext>,
        copy_info: CopyInfo,
    ) -> Result<Self, MononokeError> {
        match (base, other, copy_info) {
            (Some(base), None, CopyInfo::None) => Ok(Self::Added(base)),
            (None, Some(other), CopyInfo::None) => Ok(Self::Removed(other)),
            (Some(base), Some(other), CopyInfo::None) => Ok(Self::Changed(base, other)),
            (Some(base), Some(other), CopyInfo::Copy) => Ok(Self::Copied(base, other)),
            (Some(base), Some(other), CopyInfo::Move) => Ok(Self::Moved(base, other)),
            invalid_args => Err(anyhow!(
                "Invalid changeset path diff context parameters: {:?}",
                invalid_args
            )
            .into()),
        }
    }

    /// Return the base path that is being compared.  This is the
    /// contents after modification.
    pub fn base(&self) -> Option<&ChangesetPathContentContext> {
        match self {
            Self::Added(base)
            | Self::Changed(base, _)
            | Self::Copied(base, _)
            | Self::Moved(base, _) => Some(base),
            Self::Removed(_) => None,
        }
    }

    /// Return the other path that is being compared against.  This
    /// is the contents before modification.
    pub fn other(&self) -> Option<&ChangesetPathContentContext> {
        match self {
            Self::Removed(other)
            | Self::Changed(_, other)
            | Self::Copied(_, other)
            | Self::Moved(_, other) => Some(other),
            Self::Added(_) => None,
        }
    }

    /// Return the main path for this difference.  This is the added or
    /// removed path, or the base (destination) in the case of modifications,
    /// copies, or moves.
    pub fn path(&self) -> &ChangesetPathContentContext {
        match self {
            Self::Added(base)
            | Self::Changed(base, _)
            | Self::Copied(base, _)
            | Self::Moved(base, _) => base,
            Self::Removed(other) => other,
        }
    }

    /// Return the copy information for this difference.
    pub fn copy_info(&self) -> CopyInfo {
        match self {
            Self::Added(_) | Self::Removed(_) | Self::Changed(_, _) => CopyInfo::None,
            Self::Copied(_, _) => CopyInfo::Copy,
            Self::Moved(_, _) => CopyInfo::Move,
        }
    }

    // Helper for getting file information.
    async fn get_file_data(
        path: Option<&ChangesetPathContentContext>,
        mode: UnifiedDiffMode,
    ) -> Result<Option<xdiff::DiffFile<String, Bytes>>, MononokeError> {
        match path {
            Some(path) => {
                if let Some(file_type) = path.file_type().await? {
                    let file = path.file().await?.ok_or_else(|| {
                        MononokeError::from(Error::msg("assertion error: file should exist"))
                    })?;
                    let file_type = match file_type {
                        FileType::Regular => xdiff::FileType::Regular,
                        FileType::Executable => xdiff::FileType::Executable,
                        FileType::Symlink => xdiff::FileType::Symlink,
                    };
                    let contents = match mode {
                        UnifiedDiffMode::Inline => {
                            let contents = file.content_concat().await?;
                            xdiff::FileContent::Inline(contents)
                        }
                        UnifiedDiffMode::OmitContent => {
                            let content_id = file.metadata().await?.content_id;
                            xdiff::FileContent::Omitted {
                                content_hash: format!("{}", content_id),
                            }
                        }
                    };
                    Ok(Some(xdiff::DiffFile {
                        path: path.path().to_string(),
                        contents,
                        file_type,
                    }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Renders the diff (in the git diff format).
    ///
    /// If `mode` is `Placeholder` then `unified_diff(...)` doesn't fetch content,
    /// but just generates a placeholder diff that says that the files differ.
    pub async fn unified_diff(
        &self,
        context_lines: usize,
        mode: UnifiedDiffMode,
    ) -> Result<UnifiedDiff, MononokeError> {
        let (base_file, other_file) = try_join!(
            Self::get_file_data(self.base(), mode),
            Self::get_file_data(self.other(), mode)
        )?;
        let is_binary = xdiff::file_is_binary(&base_file) || xdiff::file_is_binary(&other_file);
        let copy_info = self.copy_info();
        let opts = xdiff::DiffOpts {
            context: context_lines,
            copy_info,
        };
        // The base is the target, so we diff in the opposite direction.
        let raw_diff = xdiff::diff_unified(other_file, base_file, opts);
        Ok(UnifiedDiff {
            raw_diff,
            is_binary,
        })
    }
}
