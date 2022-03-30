/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::path::Path;

use dag::errors::NotFoundError;
// TODO: Consider migrating to async, or get rid of strip in tests.
use dag::nameset::SyncNameSetQuery;
use dag::DagAlgorithm;
use dag::Set;
use dag::Vertex;

use crate::AppendCommits;
use crate::HgCommit;
use crate::ReadCommitText;
use crate::Result;

#[async_trait::async_trait]
pub trait StripCommits {
    /// Strip commits. This is for legacy tests only that wouldn't be used
    /// much in production. The callsite should take care of locking or
    /// otherwise risk data race and loss.
    async fn strip_commits(&mut self, set: Set) -> Result<()>;
}

/// Enumerate all commits in `orig`, re-insert them to `new` except for `strip_set::`.
/// This has unacceptable time complexity so it can only be used in tests.
pub(crate) async fn migrate_commits(
    orig: &(impl ReadCommitText + DagAlgorithm),
    new: &mut impl AppendCommits,
    strip_set: Set,
) -> Result<()> {
    if std::env::var_os("TESTTMP").is_none() {
        return Err(crate::errors::test_only("strip"));
    }
    let set = orig.all().await? - orig.descendants(strip_set).await?;
    let heads: Vec<Vertex> = orig
        .heads(set.clone())
        .await?
        .iter_rev()?
        .collect::<dag::Result<Vec<_>>>()?;
    let mut commits: Vec<HgCommit> = Vec::with_capacity(set.count()?);
    // This is inefficient - one by one fetching via async.
    // However the strip code paths only exist to support legacy `.t`
    // tests that use real strips. It's not used anywhere in production.
    // So no optimization is done here.
    for vertex in set.iter_rev()? {
        let vertex = vertex?;
        let raw_text = match orig.get_commit_raw_text(&vertex).await? {
            Some(text) => text,
            None => return Err(vertex.not_found_error().into()),
        };
        let parents = orig.parent_names(vertex.clone()).await?;
        let commit = HgCommit {
            vertex,
            parents,
            raw_text,
        };
        commits.push(commit);
    }
    new.add_commits(&commits).await?;
    new.flush(&heads).await?;
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
