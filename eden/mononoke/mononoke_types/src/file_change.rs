/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use edenapi_types::FileType as EdenapiFileType;
use quickcheck::empty_shrinker;
use quickcheck::single_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::errors::MononokeTypeError;
use crate::path::NonRootMPath;
use crate::thrift;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::ContentId;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TrackedFileChange {
    inner: BasicFileChange,
    copy_from: Option<(NonRootMPath, ChangesetId)>,
}

#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize
)]
/// Controls file representation when served over Git protocol
pub enum GitLfs {
    /// Full contents of the file should be served over Git protocol
    #[default]
    FullContent,
    /// A Git-LFS pointer should be served over git protocol
    GitLfsPointer {
        /// The content id of the pointer if different from the default
        /// one created by Git LFS or Mononoke.
        non_canonical_pointer: Option<ContentId>,
    },
}

impl GitLfs {
    pub fn full_content() -> Self {
        Self::FullContent
    }

    pub fn canonical_pointer() -> Self {
        Self::GitLfsPointer {
            non_canonical_pointer: None,
        }
    }

    pub fn non_canonical_pointer(content_id: ContentId) -> Self {
        Self::GitLfsPointer {
            non_canonical_pointer: Some(content_id),
        }
    }

    pub fn is_lfs_pointer(&self) -> bool {
        match self {
            GitLfs::GitLfsPointer { .. } => true,
            GitLfs::FullContent => false,
        }
    }

    pub fn non_canonical_pointer_content_id(&self) -> Option<ContentId> {
        match self {
            GitLfs::GitLfsPointer {
                non_canonical_pointer,
            } => *non_canonical_pointer,
            GitLfs::FullContent => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BasicFileChange {
    content_id: ContentId,
    file_type: FileType,
    size: u64,
    git_lfs: GitLfs,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FileChange {
    Change(TrackedFileChange),
    Deletion,
    // TODO(T98053352): Possibly put copy information on untracked changes
    UntrackedChange(BasicFileChange),
    UntrackedDeletion,
}

impl TrackedFileChange {
    pub fn new(
        content_id: ContentId,
        file_type: FileType,
        size: u64,
        copy_from: Option<(NonRootMPath, ChangesetId)>,
        git_lfs: GitLfs,
    ) -> Self {
        Self {
            inner: BasicFileChange {
                content_id,
                file_type,
                size,
                git_lfs,
            },
            copy_from,
        }
    }

    pub fn with_new_copy_from(&self, copy_from: Option<(NonRootMPath, ChangesetId)>) -> Self {
        Self::new(
            self.inner.content_id,
            self.inner.file_type,
            self.inner.size,
            copy_from,
            self.inner.git_lfs,
        )
    }

    /// Drops the Git-LFS information from file change
    /// useful when mirroring commits to repos that don't support Git-LFS.
    pub fn without_git_lfs(&self) -> Self {
        Self::new(
            self.inner.content_id,
            self.inner.file_type,
            self.inner.size,
            self.copy_from.clone(),
            GitLfs::FullContent,
        )
    }

    pub(crate) fn into_thrift(self) -> thrift::bonsai::FileChange {
        thrift::bonsai::FileChange {
            content_id: self.inner.content_id.into_thrift(),
            file_type: self.inner.file_type.into_thrift(),
            size: self.inner.size as i64,
            copy_from: self
                .copy_from
                .map(|(file, cs_id)| thrift::bonsai::CopyInfo {
                    file: file.into_thrift(),
                    cs_id: cs_id.into_thrift(),
                }),
            git_lfs: match self.inner.git_lfs {
                GitLfs::GitLfsPointer {
                    non_canonical_pointer,
                } => Some(thrift::bonsai::GitLfs {
                    non_canonical_pointer_content_id: non_canonical_pointer
                        .map(|id| id.into_thrift()),
                    ..Default::default()
                }),
                GitLfs::FullContent => None,
            },
        }
    }

    pub fn content_id(&self) -> ContentId {
        self.inner.content_id
    }

    pub fn file_type(&self) -> FileType {
        self.inner.file_type
    }

    pub fn git_lfs(&self) -> GitLfs {
        self.inner.git_lfs
    }

    pub fn size(&self) -> u64 {
        self.inner.size
    }

    pub fn copy_from(&self) -> Option<&(NonRootMPath, ChangesetId)> {
        self.copy_from.as_ref()
    }

    pub fn copy_from_mut(&mut self) -> Option<&mut (NonRootMPath, ChangesetId)> {
        self.copy_from.as_mut()
    }

    pub(crate) fn from_thrift(
        fc: thrift::bonsai::FileChange,
        mpath: &NonRootMPath,
    ) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(Self {
                inner: BasicFileChange {
                    content_id: ContentId::from_thrift(fc.content_id)?,
                    file_type: FileType::from_thrift(fc.file_type)?,
                    size: fc.size as u64,
                    git_lfs: match fc.git_lfs {
                        Some(git_lfs) => GitLfs::GitLfsPointer {
                            non_canonical_pointer: git_lfs
                                .non_canonical_pointer_content_id
                                .map(ContentId::from_thrift)
                                .transpose()?,
                        },
                        None => GitLfs::FullContent,
                    },
                },
                copy_from: match fc.copy_from {
                    Some(copy_info) => Some((
                        NonRootMPath::from_thrift(copy_info.file)?,
                        ChangesetId::from_thrift(copy_info.cs_id)?,
                    )),
                    None => None,
                },
            })
        };

        catch_block().with_context(|| {
            MononokeTypeError::InvalidThrift(
                "FileChange".into(),
                format!("Invalid changed entry for path {}", mpath),
            )
        })
    }
}

impl BasicFileChange {
    pub fn new(content_id: ContentId, file_type: FileType, size: u64, git_lfs: GitLfs) -> Self {
        Self {
            content_id,
            file_type,
            size,
            git_lfs,
        }
    }

    pub fn content_id(&self) -> ContentId {
        self.content_id
    }

    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    pub fn git_lfs(&self) -> GitLfs {
        self.git_lfs
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn into_thrift_untracked(self) -> thrift::bonsai::UntrackedFileChange {
        thrift::bonsai::UntrackedFileChange {
            content_id: self.content_id.into_thrift(),
            file_type: self.file_type.into_thrift(),
            size: self.size as i64,
        }
    }

    pub(crate) fn from_thrift_untracked(uc: thrift::bonsai::UntrackedFileChange) -> Result<Self> {
        Ok(Self {
            content_id: ContentId::from_thrift(uc.content_id)?,
            file_type: FileType::from_thrift(uc.file_type)?,
            size: uc.size as u64,
            git_lfs: GitLfs::FullContent,
        })
    }
}

impl FileChange {
    pub fn tracked(
        content_id: ContentId,
        file_type: FileType,
        size: u64,
        copy_from: Option<(NonRootMPath, ChangesetId)>,
        git_lfs: GitLfs,
    ) -> Self {
        Self::Change(TrackedFileChange::new(
            content_id, file_type, size, copy_from, git_lfs,
        ))
    }

    pub fn untracked(content_id: ContentId, file_type: FileType, size: u64) -> Self {
        Self::UntrackedChange(BasicFileChange {
            content_id,
            file_type,
            size,
            git_lfs: GitLfs::FullContent,
        })
    }

    /// Convert this to a simple file change, where tracked and untracked
    /// changes are treated the same way, as well as missing and deleted
    pub fn simplify(&self) -> Option<&BasicFileChange> {
        match self {
            Self::Change(tc) => Some(&tc.inner),
            Self::UntrackedChange(uc) => Some(uc),
            Self::Deletion | Self::UntrackedDeletion => None,
        }
    }

    pub fn copy_from(&self) -> Option<&(NonRootMPath, ChangesetId)> {
        match self {
            Self::Change(tc) => tc.copy_from(),
            Self::Deletion | Self::UntrackedDeletion | Self::UntrackedChange(_) => None,
        }
    }

    pub fn size(&self) -> Option<u64> {
        match &self {
            Self::Change(tc) => Some(tc.size()),
            Self::UntrackedChange(uc) => Some(uc.size),
            Self::Deletion | Self::UntrackedDeletion => None,
        }
    }

    pub fn git_lfs(&self) -> Option<GitLfs> {
        match &self {
            Self::Change(tc) => Some(tc.git_lfs()),
            Self::UntrackedChange(uc) => Some(uc.git_lfs),
            Self::Deletion | Self::UntrackedDeletion => None,
        }
    }

    pub fn is_changed(&self) -> bool {
        match &self {
            Self::Change(_) | Self::UntrackedChange(_) => true,
            Self::Deletion | Self::UntrackedDeletion => false,
        }
    }

    pub fn is_removed(&self) -> bool {
        match &self {
            Self::Change(_) | Self::UntrackedChange(_) => false,
            Self::Deletion | Self::UntrackedDeletion => true,
        }
    }

    pub(crate) fn from_thrift(
        fc_opt: thrift::bonsai::FileChangeOpt,
        mpath: &NonRootMPath,
    ) -> Result<Self> {
        match (
            fc_opt.change,
            fc_opt.untracked_change,
            fc_opt.untracked_deletion,
        ) {
            (Some(tc), None, None) => Ok(Self::Change(TrackedFileChange::from_thrift(tc, mpath)?)),
            (None, Some(uc), None) => Ok(Self::UntrackedChange(
                BasicFileChange::from_thrift_untracked(uc)?,
            )),
            (None, None, Some(_)) => Ok(Self::UntrackedDeletion),
            (None, None, None) => Ok(Self::Deletion),
            _ => bail!("FileChangeOpt has more than one present field"),
        }
    }

    #[inline]
    pub(crate) fn into_thrift(self) -> thrift::bonsai::FileChangeOpt {
        let mut fco = thrift::bonsai::FileChangeOpt {
            change: None,
            untracked_change: None,
            untracked_deletion: None,
        };
        match self {
            Self::Change(tc) => {
                fco.change = Some(tc.into_thrift());
            }
            Self::UntrackedChange(uc) => {
                fco.untracked_change = Some(uc.into_thrift_untracked());
            }
            Self::UntrackedDeletion => {
                fco.untracked_deletion = Some(thrift::bonsai::UntrackedDeletion {});
            }
            Self::Deletion => {}
        }
        fco
    }

    /// Generate a random FileChange which picks copy-from parents from the list of parents
    /// provided.
    pub(crate) fn arbitrary_from_parents(g: &mut Gen, parents: &[ChangesetId]) -> Self {
        let copy_from = if *g.choose(&[0, 1, 2, 3, 4]).unwrap() < 1 {
            g.choose(parents)
                .map(|parent| (NonRootMPath::arbitrary(g), *parent))
        } else {
            None
        };
        Self::Change(TrackedFileChange::new(
            ContentId::arbitrary(g),
            FileType::arbitrary(g),
            u64::arbitrary(g),
            copy_from,
            GitLfs::FullContent,
        ))
    }
}

impl Arbitrary for FileChange {
    fn arbitrary(g: &mut Gen) -> Self {
        let copy_from = if *g.choose(&[0, 1, 2, 3, 4]).unwrap() < 1 {
            Some((NonRootMPath::arbitrary(g), ChangesetId::arbitrary(g)))
        } else {
            None
        };
        Self::Change(TrackedFileChange::new(
            ContentId::arbitrary(g),
            FileType::arbitrary(g),
            u64::arbitrary(g),
            copy_from,
            GitLfs::FullContent,
        ))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        // The only thing that can be reduced here is copy_from.
        if let Self::Change(tc) = self {
            if tc.copy_from.is_some() {
                single_shrinker(Self::Change(TrackedFileChange {
                    copy_from: None,
                    ..tc.clone()
                }))
            } else {
                empty_shrinker()
            }
        } else {
            empty_shrinker()
        }
    }
}

/// Type of a file.
///
/// Regular and Executable are identical - they both represent files containing arbitrary content.
/// The only difference is that the Executables are created with executable permission when
/// checked out.
///
/// Symlink is also the same as Regular, but the file content is used to create a symbolic link
/// (or equivalent) when checked out.  Mononoke never interpolates symlinks itself, as they are
/// not guaranteed to resolve to a file in the repo, or to anything valid at all.
///
/// GitSubmodule represents a submodule in a git tree.  The file content contains the binary hash
/// of the git commit that the submodule currently refers to.  Mononoke does not interpret Git
/// submodules; they must be interpreted by the client during checkout based on local repo
/// configuration.
#[derive(
    Debug,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub enum FileType {
    Regular,
    Executable,
    Symlink,
    GitSubmodule,
}

impl FileType {
    /// All possible file types.
    pub fn all() -> [FileType; 4] {
        [
            FileType::Regular,
            FileType::Executable,
            FileType::Symlink,
            FileType::GitSubmodule,
        ]
    }

    /// All the file types that `self` is not.
    pub fn complement(&self) -> [FileType; 3] {
        use FileType::*;
        match self {
            Regular => [Executable, Symlink, GitSubmodule],
            Executable => [Regular, Symlink, GitSubmodule],
            Symlink => [Regular, Executable, GitSubmodule],
            GitSubmodule => [Regular, Executable, Symlink],
        }
    }

    pub fn from_thrift(ft: thrift::bonsai::FileType) -> Result<Self> {
        let file_type = match ft {
            thrift::bonsai::FileType::Regular => FileType::Regular,
            thrift::bonsai::FileType::Executable => FileType::Executable,
            thrift::bonsai::FileType::Symlink => FileType::Symlink,
            thrift::bonsai::FileType::GitSubmodule => FileType::GitSubmodule,
            thrift::bonsai::FileType(x) => bail!(MononokeTypeError::InvalidThrift(
                "FileType".into(),
                format!("unknown file type '{}'", x)
            )),
        };
        Ok(file_type)
    }

    pub fn into_thrift(self) -> thrift::bonsai::FileType {
        match self {
            FileType::Regular => thrift::bonsai::FileType::Regular,
            FileType::Executable => thrift::bonsai::FileType::Executable,
            FileType::Symlink => thrift::bonsai::FileType::Symlink,
            FileType::GitSubmodule => thrift::bonsai::FileType::GitSubmodule,
        }
    }
}

impl TryFrom<FileType> for EdenapiFileType {
    type Error = MononokeTypeError;

    fn try_from(v: FileType) -> Result<Self, Self::Error> {
        use EdenapiFileType::*;
        match v {
            FileType::Regular => Ok(Regular),
            FileType::Executable => Ok(Executable),
            FileType::Symlink => Ok(Symlink),
            FileType::GitSubmodule => Err(MononokeTypeError::GitSubmoduleNotSupported),
        }
    }
}

impl From<EdenapiFileType> for FileType {
    fn from(v: EdenapiFileType) -> Self {
        use EdenapiFileType::*;
        match v {
            Regular => FileType::Regular,
            Executable => FileType::Executable,
            Symlink => FileType::Symlink,
        }
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            FileType::Symlink => "symlink",
            FileType::Executable => "executable",
            FileType::Regular => "regular",
            FileType::GitSubmodule => "git-submodule",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for FileType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "regular" | "file" => Ok(FileType::Regular),
            "executable" | "exec" => Ok(FileType::Executable),
            "symlink" | "link" => Ok(FileType::Symlink),
            "git-submodule" | "gitm" => Ok(FileType::GitSubmodule),
            _ => bail!("Invalid file type: {s}"),
        }
    }
}

impl FromStr for GitLfs {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "full_content" => Ok(GitLfs::full_content()),
            "lfs_pointer" | "lfs" => Ok(GitLfs::canonical_pointer()),
            _ => bail!("Invalid GitLfs flag: {s}"),
        }
    }
}

impl Arbitrary for FileType {
    fn arbitrary(g: &mut Gen) -> Self {
        match u64::arbitrary(g) % 100 {
            0..=9 => FileType::Executable,
            10..=19 => FileType::Symlink,
            20 => FileType::GitSubmodule,
            _ => FileType::Regular,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        empty_shrinker()
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn filetype_thrift_roundtrip(ft: FileType) -> bool {
            let thrift_ft = ft.into_thrift();
            let ft2 = FileType::from_thrift(thrift_ft)
                .expect("thrift roundtrip should always be valid");
            ft == ft2
        }

        fn filechange_thrift_roundtrip(fc: FileChange) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = FileChange::from_thrift(thrift_fc, &NonRootMPath::new("foo").unwrap())
                .expect("thrift roundtrip should always be valid");
            fc == fc2
        }
    }

    #[mononoke::test]
    fn bad_filetype_thrift() {
        let thrift_ft = thrift::bonsai::FileType(42);
        FileType::from_thrift(thrift_ft).expect_err("unexpected OK - unknown file type");
    }

    #[mononoke::test]
    fn bad_filechange_thrift() {
        let thrift_fc = thrift::bonsai::FileChange {
            content_id: thrift::id::ContentId(thrift::id::Id::Blake2(thrift::id::Blake2(
                vec![0; 16].into(),
            ))),
            file_type: thrift::bonsai::FileType::Regular,
            size: 0,
            copy_from: None,
            git_lfs: None,
        };
        TrackedFileChange::from_thrift(thrift_fc, &NonRootMPath::new("foo").unwrap())
            .expect_err("unexpected OK - bad content ID");
    }
}
