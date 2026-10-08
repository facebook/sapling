/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Display;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::GitLfs;
use mononoke_types::SubtreeChange;
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum DisplaySubtreeChange {
    SubtreeCopy {
        from_path: String,
        from_cs_id: ChangesetId,
    },
    SubtreeDeepCopy {
        from_path: String,
        from_cs_id: ChangesetId,
    },
    SubtreeMerge {
        from_path: String,
        from_cs_id: ChangesetId,
    },
    SubtreeImport {
        from_path: String,
        from_commit: String,
        from_repo_url: String,
    },
    SubtreeCrossRepoMerge {
        from_path: String,
        from_commit: String,
        from_repo_url: String,
    },
}

impl DisplaySubtreeChange {
    fn from_subtree_change(change: &SubtreeChange) -> Self {
        match change {
            SubtreeChange::SubtreeCopy(copy) => DisplaySubtreeChange::SubtreeCopy {
                from_path: copy.from_path.to_string(),
                from_cs_id: copy.from_cs_id,
            },
            SubtreeChange::SubtreeDeepCopy(copy) => DisplaySubtreeChange::SubtreeDeepCopy {
                from_path: copy.from_path.to_string(),
                from_cs_id: copy.from_cs_id,
            },
            SubtreeChange::SubtreeMerge(merge) => DisplaySubtreeChange::SubtreeMerge {
                from_path: merge.from_path.to_string(),
                from_cs_id: merge.from_cs_id,
            },
            SubtreeChange::SubtreeImport(import) => DisplaySubtreeChange::SubtreeImport {
                from_path: import.from_path.to_string(),
                from_commit: import.from_commit.clone(),
                from_repo_url: import.from_repo_url.clone(),
            },
            SubtreeChange::SubtreeCrossRepoMerge(merge) => {
                DisplaySubtreeChange::SubtreeCrossRepoMerge {
                    from_path: merge.from_path.to_string(),
                    from_commit: merge.from_commit.clone(),
                    from_repo_url: merge.from_repo_url.clone(),
                }
            }
        }
    }
}

#[derive(Serialize)]
pub struct DisplayChangeset {
    pub changeset_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub hg_extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<String, FileChange>,
    pub subtree_changes: BTreeMap<String, DisplaySubtreeChange>,
}

impl TryFrom<&BonsaiChangeset> for DisplayChangeset {
    type Error = Error;

    fn try_from(bonsai: &BonsaiChangeset) -> Result<Self> {
        Ok(DisplayChangeset {
            changeset_id: bonsai.get_changeset_id(),
            parents: bonsai.parents().collect(),
            author: bonsai.author().to_string(),
            author_date: bonsai.author_date().clone(),
            committer: bonsai.committer().map(ToString::to_string),
            committer_date: bonsai.committer_date().cloned(),
            message: bonsai.message().to_string(),
            hg_extra: bonsai
                .hg_extra()
                .map(|(k, v)| (k.to_string(), v.to_vec()))
                .collect(),
            file_changes: bonsai
                .file_changes_map()
                .iter()
                .map(|(k, v)| {
                    Ok((
                        String::from_utf8(k.to_vec())
                            .with_context(|| format!("Invalid extra name: {k:?}"))?,
                        v.clone(),
                    ))
                })
                .collect::<Result<_>>()?,
            subtree_changes: bonsai
                .subtree_changes()
                .iter()
                .map(|(k, v)| (k.to_string(), DisplaySubtreeChange::from_subtree_change(v)))
                .collect(),
        })
    }
}

impl Display for DisplayChangeset {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "BonsaiChangesetId: {}", self.changeset_id)?;
        writeln!(fmt, "Author: {}", self.author)?;
        writeln!(fmt, "Message: {}", self.message)?;
        writeln!(fmt, "FileChanges:")?;
        for (path, change) in self.file_changes.iter() {
            writeln!(fmt, "{}", display_file_change(path, change))?;
        }
        if !self.subtree_changes.is_empty() {
            writeln!(fmt, "SubtreeChanges:")?;
            for (path, change) in self.subtree_changes.iter() {
                writeln!(fmt, "{}", display_subtree_change(path, change))?;
            }
        }
        Ok(())
    }
}

pub fn display_file_change(path: &String, change: &FileChange) -> String {
    match change {
        FileChange::Change(change) => {
            let lfs = match change.git_lfs() {
                GitLfs::FullContent => "".to_string(),
                GitLfs::GitLfsPointer {
                    non_canonical_pointer: None,
                } => " (LFS)".to_string(),
                GitLfs::GitLfsPointer {
                    non_canonical_pointer: Some(id),
                } => format!(" (LFS, non-canonical pointer: {id})"),
            };
            match change.copy_from() {
                Some(_) => format!("\t COPY/MOVE{}: {} {}", lfs, path, change.content_id()),
                None => format!("\t ADDED/MODIFIED{}: {} {}", lfs, path, change.content_id()),
            }
        }
        FileChange::Deletion => format!("\t REMOVED: {path}"),
        FileChange::UntrackedChange(change) => {
            format!("\t UNTRACKED ADD/MODIFY: {} {}", path, change.content_id())
        }
        FileChange::UntrackedDeletion => format!("\t MISSING: {path}"),
    }
}

pub fn display_subtree_change(path: &String, change: &DisplaySubtreeChange) -> String {
    match change {
        DisplaySubtreeChange::SubtreeCopy {
            from_path,
            from_cs_id,
        } => format!("\t SUBTREE_COPY: {path} (from {from_path} @ {from_cs_id})"),
        DisplaySubtreeChange::SubtreeDeepCopy {
            from_path,
            from_cs_id,
        } => format!("\t SUBTREE_DEEP_COPY: {path} (from {from_path} @ {from_cs_id})"),
        DisplaySubtreeChange::SubtreeMerge {
            from_path,
            from_cs_id,
        } => format!("\t SUBTREE_MERGE: {path} (from {from_path} @ {from_cs_id})"),
        DisplaySubtreeChange::SubtreeImport {
            from_path,
            from_commit,
            from_repo_url,
        } => format!(
            "\t SUBTREE_IMPORT: {path} (from {from_path} @ {from_commit} in {from_repo_url})"
        ),
        DisplaySubtreeChange::SubtreeCrossRepoMerge {
            from_path,
            from_commit,
            from_repo_url,
        } => format!(
            "\t SUBTREE_CROSS_REPO_MERGE: {path} (from {from_path} @ {from_commit} in {from_repo_url})"
        ),
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn display_changeset(hg_extra: BTreeMap<String, Vec<u8>>) -> Result<DisplayChangeset> {
        Ok(DisplayChangeset {
            changeset_id: ChangesetId::from_bytes([1u8; 32])?,
            parents: vec![],
            author: "author".to_string(),
            author_date: DateTime::from_timestamp(0, 0)?,
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            hg_extra,
            file_changes: BTreeMap::new(),
            subtree_changes: BTreeMap::new(),
        })
    }

    /// FIXME: BUG! This did serialize `hg_extra` values as JSON arrays of byte values
    /// (`"hg-git-rename-source": [103, 105, 116]`) in `mononoke_admin fetch --json`, but should
    /// have rendered a value that is valid UTF-8 as a JSON string (`"hg-git-rename-source":
    /// "git"`) and a value that is not as a `{"hex": "<hex>"}` object, like every other field of
    /// the JSON output. This test pins the current behaviour; the next diff fixes it.
    #[mononoke::test]
    fn hg_extra_json_rendering() -> Result<()> {
        let changeset = display_changeset(BTreeMap::from([
            ("hg-git-rename-source".to_string(), b"git".to_vec()),
            ("convert_revision".to_string(), b"1234abcd".to_vec()),
            (
                "transplant_source".to_string(),
                vec![0x31, 0x32, 0x33, 0xff],
            ),
        ]))?;
        let json = serde_json::to_value(&changeset)?;
        // FIXME: BUG! These should be the strings "git" and "1234abcd" and the object
        // {"hex": "313233ff"}.
        assert_eq!(
            json["hg_extra"]["hg-git-rename-source"],
            serde_json::json!([103, 105, 116])
        );
        assert_eq!(
            json["hg_extra"]["convert_revision"],
            serde_json::json!([49, 50, 51, 52, 97, 98, 99, 100])
        );
        assert_eq!(
            json["hg_extra"]["transplant_source"],
            serde_json::json!([49, 50, 51, 255])
        );
        Ok(())
    }
}
