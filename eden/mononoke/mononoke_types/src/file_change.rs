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

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BasicFileChange {
    content_id: ContentId,
    file_type: FileType,
    size: u64,
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
    ) -> Self {
        Self {
            inner: BasicFileChange {
                content_id,
                file_type,
                size,
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
        )
    }

    pub(crate) fn into_thrift(self) -> thrift::FileChange {
        thrift::FileChange {
            content_id: self.inner.content_id.into_thrift(),
            file_type: self.inner.file_type.into_thrift(),
            size: self.inner.size as i64,
            copy_from: self.copy_from.map(|(file, cs_id)| thrift::CopyInfo {
                file: file.into_thrift(),
                cs_id: cs_id.into_thrift(),
            }),
        }
    }

    pub fn content_id(&self) -> ContentId {
        self.inner.content_id
    }

    pub fn file_type(&self) -> FileType {
        self.inner.file_type
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

    pub(crate) fn from_thrift(fc: thrift::FileChange, mpath: &NonRootMPath) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(Self {
                inner: BasicFileChange {
                    content_id: ContentId::from_thrift(fc.content_id)?,
                    file_type: FileType::from_thrift(fc.file_type)?,
                    size: fc.size as u64,
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
    pub fn new(content_id: ContentId, file_type: FileType, size: u64) -> Self {
        Self {
            content_id,
            file_type,
            size,
        }
    }

    pub fn content_id(&self) -> ContentId {
        self.content_id
    }

    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn into_thrift_untracked(self) -> thrift::UntrackedFileChange {
        thrift::UntrackedFileChange {
            content_id: self.content_id.into_thrift(),
            file_type: self.file_type.into_thrift(),
            size: self.size as i64,
        }
    }

    pub(crate) fn from_thrift_untracked(uc: thrift::UntrackedFileChange) -> Result<Self> {
        Ok(Self {
            content_id: ContentId::from_thrift(uc.content_id)?,
            file_type: FileType::from_thrift(uc.file_type)?,
            size: uc.size as u64,
        })
    }
}

impl FileChange {
    pub fn tracked(
        content_id: ContentId,
        file_type: FileType,
        size: u64,
        copy_from: Option<(NonRootMPath, ChangesetId)>,
    ) -> Self {
        Self::Change(TrackedFileChange::new(
            content_id, file_type, size, copy_from,
        ))
    }

    pub fn untracked(content_id: ContentId, file_type: FileType, size: u64) -> Self {
        Self::UntrackedChange(BasicFileChange {
            content_id,
            file_type,
            size,
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

    pub(crate) fn from_thrift(fc_opt: thrift::FileChangeOpt, mpath: &NonRootMPath) -> Result<Self> {
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
    pub(crate) fn into_thrift(self) -> thrift::FileChangeOpt {
        let mut fco = thrift::FileChangeOpt {
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
                fco.untracked_deletion = Some(thrift::UntrackedDeletion {});
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

    pub fn from_thrift(ft: thrift::FileType) -> Result<Self> {
        let file_type = match ft {
            thrift::FileType::Regular => FileType::Regular,
            thrift::FileType::Executable => FileType::Executable,
            thrift::FileType::Symlink => FileType::Symlink,
            thrift::FileType::GitSubmodule => FileType::GitSubmodule,
            thrift::FileType(x) => bail!(MononokeTypeError::InvalidThrift(
                "FileType".into(),
                format!("unknown file type '{}'", x)
            )),
        };
        Ok(file_type)
    }

    pub fn into_thrift(self) -> thrift::FileType {
        match self {
            FileType::Regular => thrift::FileType::Regular,
            FileType::Executable => thrift::FileType::Executable,
            FileType::Symlink => thrift::FileType::Symlink,
            FileType::GitSubmodule => thrift::FileType::GitSubmodule,
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

    #[test]
    fn bad_filetype_thrift() {
        let thrift_ft = thrift::FileType(42);
        FileType::from_thrift(thrift_ft).expect_err("unexpected OK - unknown file type");
    }

    #[test]
    fn bad_filechange_thrift() {
        let thrift_fc = thrift::FileChange {
            content_id: thrift::ContentId(thrift::IdType::Blake2(thrift::Blake2(
                vec![0; 16].into(),
            ))),
            file_type: thrift::FileType::Regular,
            size: 0,
            copy_from: None,
        };
        TrackedFileChange::from_thrift(thrift_fc, &NonRootMPath::new("foo").unwrap())
            .expect_err("unexpected OK - bad content ID");
    }
}
