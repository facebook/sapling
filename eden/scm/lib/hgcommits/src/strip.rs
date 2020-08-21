/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::AppendCommits;
use crate::HgCommit;
use crate::ReadCommitText;
use crate::Result;
use dag::DagAlgorithm;
use dag::Set;
use dag::Vertex;
use std::fs;
use std::path::Path;

pub trait StripCommits {
    /// Strip commits. This is for legacy tests only that wouldn't be used
    /// much in production. The callsite should take care of locking or
    /// otherwise risk data race and loss.
    fn strip_commits(&mut self, set: Set) -> Result<()>;
}

/// Enumerate all commits in `orig`, re-insert them to `new` except for `strip_set::`.
pub(crate) fn migrate_commits(
    orig: &(impl ReadCommitText + DagAlgorithm),
    new: &mut impl AppendCommits,
    strip_set: Set,
) -> Result<()> {
    if std::env::var_os("TESTTMP").is_none() {
        return Err(crate::errors::test_only("strip"));
    }
    let set = orig.all()? - orig.descendants(strip_set)?;
    let heads: Vec<Vertex> = orig
        .heads(set.clone())?
        .iter_rev()?
        .collect::<dag::Result<Vec<_>>>()?;
    let commits: Vec<HgCommit> = set
        .iter_rev()?
        .map(|vertex| -> Result<HgCommit> {
            let vertex = vertex?;
            let raw_text = match orig.get_commit_raw_text(&vertex)? {
                Some(text) => text,
                None => return Err(vertex.not_found_error().into()),
            };
            let parents = orig.parent_names(vertex.clone())?;
            Ok(HgCommit {
                vertex,
                parents,
                raw_text,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    new.add_commits(&commits)?;
    new.flush(&heads)?;
    Ok(())
}

/// Move files and directories in `src_dir` to `dst_dir`.
/// Existing files are moved to `old.${epoch}`.
/// Racy. Should be used in non-production setup.
pub(crate) fn racy_unsafe_move_files(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    if std::env::var_os("TESTTMP").is_none() {
        return Err(crate::errors::test_only("racy_unsafe_move_files"));
    }
    let backup_dir = {
        let mut epoch = 0;
        loop {
            let dir = dst_dir.join(format!("old.{}", epoch));
            if dir.exists() {
                epoch += 1;
            } else {
                fs::create_dir(&dir)?;
                break dir;
            }
        }
    };
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let src_path = src_dir.join(&name);
        let dst_path = dst_dir.join(&name);
        let backup_path = backup_dir.join(&name);
        fs::rename(&dst_path, backup_path)?;
        fs::rename(src_path, &dst_path)?;
    }
    Ok(())
}
