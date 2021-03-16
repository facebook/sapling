/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Check-in conflict data models.

use serde::Deserialize;
use serde::Serialize;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::ops::Add;
use std::ops::Sub;
use types::HgId;

/// Conflict state in a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitConflict {
    // Not using RepoPathBuf to avoid one nested level of serde serialization.
    #[serde(rename = "files")]
    pub files: BTreeMap<String, FileConflict>,
}

/// Conflict state in a file.
///
/// Also represent a resolution of a 3-way merge. See [`FileConflict::from_3way`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConflict {
    /// Added contexts. Usually they are the commits that have conflicts
    #[serde(rename = "adds")]
    pub adds: Vec<FileContext>,

    /// Removed contexts. Usually they are merge bases.
    #[serde(rename = "removes")]
    pub removes: Vec<FileContext>,
}

/// A version of a file (no conflict).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContext {
    /// File content identity. `None` means the file was deleted.
    #[serde(rename = "id")]
    pub id: Option<HgId>,

    /// Flags of the file. "x": executable. "l": symlink.
    ///
    /// Note: This is not useful if `id` is `None`. But it seems simpler if we
    /// introduces one less struct/enum.
    #[serde(rename = "flags", default)]
    pub flags: String,

    // Not using RepoPathBuf to avoid one nested level of serde serialization.
    /// Copy-from information.
    #[serde(rename = "copy_from", default)]
    pub copy_from: Option<String>,

    /// Commit identity. Useful to find merge base. Or show up in conflict marker.
    ///
    /// `None` means the current commit.
    #[serde(rename = "commit")]
    pub commit_id: Option<HgId>,
}

impl PartialEq for FileContext {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.flags == other.flags && self.copy_from == other.copy_from
    }
}

impl Hash for FileContext {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (&self.id, &self.flags, &self.copy_from).hash(state)
    }
}

impl FileConflict {
    /// Test if the conflict is resolved.
    pub fn is_resolved(&self) -> bool {
        self.adds.len() == 1 && self.removes.is_empty()
    }

    /// Create a `FileConflict` that represents a resolved content.
    pub fn from_file(file: FileContext) -> Self {
        Self {
            adds: vec![file],
            removes: Vec::new(),
        }
    }

    /// Create a `FileConflict` that is a conflict of a 3-way merge.
    pub fn from_3way(base: FileContext, local: FileContext, other: FileContext) -> Self {
        Self {
            adds: vec![local, other],
            removes: vec![base],
        }
        .simplify()
    }

    /// Create a "resolution" that resolves the current conflict.
    ///
    /// Can be chained to record a resolution, for example,
    /// `FileConflict::from_3way(base, local, other).with_resolution(res)`
    /// (then store it in a re-re-re storage).
    pub fn with_resolution(self, resolution: FileContext) -> Self {
        let mut adds = self.removes;
        let removes = self.adds;
        adds.push(resolution);
        Self { adds, removes }.simplify()
    }

    /// Number of 3-way merge steps needed to resolve the conflict.
    pub fn complexity(&self) -> usize {
        self.adds.len() - 1
    }

    /// Simplify internal. Cancel out same content from adds and removes.
    fn simplify(mut self) -> Self {
        for (i, add) in self.adds.clone().into_iter().enumerate().rev() {
            if let Some(j) = self.removes.iter().position(|r| r == &add) {
                // The add and remove cancel out.
                self.removes.remove(j);
                self.adds.remove(i);
            }
        }
        self
    }
}

impl Add<FileConflict> for FileConflict {
    type Output = FileConflict;

    fn add(mut self, rhs: FileConflict) -> Self::Output {
        self.adds.extend(rhs.adds);
        self.removes.extend(rhs.removes);
        self.simplify()
    }
}

impl Sub<FileConflict> for FileConflict {
    type Output = FileConflict;

    fn sub(mut self, rhs: FileConflict) -> Self::Output {
        self.adds.extend(rhs.removes);
        self.removes.extend(rhs.adds);
        self.simplify()
    }
}
