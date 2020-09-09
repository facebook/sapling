/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{
    compat::Stream01CompatExt,
    future::{FutureExt, TryFutureExt},
    stream::{StreamExt, TryStreamExt},
};
use futures_ext::{BoxFuture as OldBoxFuture, FutureExt as _, StreamExt as _};
use futures_old::{future as old_future, Future as _, Stream as _};
use manifest::{Diff, Entry, ManifestOps};
use mercurial_types::{HgFileNodeId, HgManifestId};
use mononoke_types::{FileType, MPath};
use std::collections::HashSet;

/// NOTE: To be used only for generating list of files for old, Mercurial format of Changesets.
///
/// This function is used to extract any new files that the given root manifest has provided
/// compared to the provided p1 and p2 parents.
/// A files is considered new when it was not present in neither of parent manifests or it was
/// present, but with a different content.
/// It sorts the returned Vec<MPath> in the order expected by Mercurial.
pub fn compute_changed_files(
    ctx: CoreContext,
    repo: BlobRepo,
    root: HgManifestId,
    p1: Option<HgManifestId>,
    p2: Option<HgManifestId>,
) -> OldBoxFuture<Vec<MPath>, Error> {
    match (p1, p2) {
        (None, None) => root
            .list_leaf_entries(ctx, repo.get_blobstore())
            .map(|(path, _)| path)
            .collect_to()
            .boxify(),
        (Some(manifest), None) | (None, Some(manifest)) => {
            compute_changed_files_pair(ctx, root, manifest, repo)
        }
        (Some(p1), Some(p2)) => {
            let f1 = compute_changed_files_pair(ctx.clone(), root, p1, repo.clone())
                .join(compute_changed_files_pair(
                    ctx.clone(),
                    root,
                    p2,
                    repo.clone(),
                ))
                .map(|(left, right)| left.intersection(&right).cloned().collect::<Vec<_>>());

            // Mercurial always includes removed files, we need to match this behaviour
            let f2 = {
                cloned!(ctx, repo);
                async move { compute_removed_files(&ctx, &repo, root, Some(p1)).await }
            }
            .boxed()
            .compat();
            let f3 = {
                cloned!(ctx, repo);
                async move { compute_removed_files(&ctx, &repo, root, Some(p2)).await }
            }
            .boxed()
            .compat();

            f1.join3(f2, f3)
                .map(|(ch1, ch2, ch3)| {
                    ch1.into_iter()
                        .chain(ch2.into_iter())
                        .chain(ch3.into_iter())
                        .collect::<HashSet<_>>()
                })
                .boxify()
        }
    }
    .map(|files| {
        let mut files: Vec<MPath> = files.into_iter().collect();
        files.sort_unstable_by(mercurial_mpath_comparator);

        files
    })
    .boxify()
}

fn compute_changed_files_pair(
    ctx: CoreContext,
    to: HgManifestId,
    from: HgManifestId,
    repo: BlobRepo,
) -> OldBoxFuture<HashSet<MPath>, Error> {
    from.diff(ctx, repo.get_blobstore(), to)
        .filter_map(|diff| {
            let (path, entry) = match diff {
                Diff::Added(path, entry) | Diff::Removed(path, entry) => (path, entry),
                Diff::Changed(path, .., entry) => (path, entry),
            };

            match entry {
                Entry::Tree(_) => None,
                Entry::Leaf(_) => path,
            }
        })
        .fold(HashSet::new(), |mut set, path| {
            set.insert(path);
            old_future::ok::<_, Error>(set)
        })
        .boxify()
}

async fn compute_removed_files(
    ctx: &CoreContext,
    repo: &BlobRepo,
    child: HgManifestId,
    parent: Option<HgManifestId>,
) -> Result<Vec<MPath>, Error> {
    compute_files_with_status(ctx, repo, child, parent, move |diff| match diff {
        Diff::Removed(path, entry) => match entry {
            Entry::Leaf(_) => path,
            Entry::Tree(_) => None,
        },
        _ => None,
    })
    .await
}

async fn compute_files_with_status(
    ctx: &CoreContext,
    repo: &BlobRepo,
    child: HgManifestId,
    parent: Option<HgManifestId>,
    filter_map: impl Fn(Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>) -> Option<MPath>,
) -> Result<Vec<MPath>, Error> {
    let s = match parent {
        Some(parent) => parent
            .diff(ctx.clone(), repo.get_blobstore(), child)
            .compat()
            .left_stream(),
        None => child
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .map(|(path, entry)| Diff::Added(path, entry))
            .compat()
            .right_stream(),
    };

    s.try_filter_map(|e| async { Ok(filter_map(e)) })
        .try_collect()
        .await
}

fn mercurial_mpath_comparator(a: &MPath, b: &MPath) -> ::std::cmp::Ordering {
    a.to_vec().cmp(&b.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mercurial_mpath_comparator() {
        let mut paths = vec![
            "foo/bar/baz/a.test",
            "foo/bar/baz-boo/a.test",
            "foo-faz/bar/baz/a.test",
        ];

        let mut mpaths: Vec<_> = paths
            .iter()
            .map(|path| MPath::new(path).expect("invalid path"))
            .collect();

        {
            mpaths.sort_unstable();
            let result: Vec<_> = mpaths
                .iter()
                .map(|mpath| String::from_utf8(mpath.to_vec()).unwrap())
                .collect();
            assert!(paths == result);
        }

        {
            paths.sort_unstable();
            mpaths.sort_unstable_by(mercurial_mpath_comparator);
            let result: Vec<_> = mpaths
                .iter()
                .map(|mpath| String::from_utf8(mpath.to_vec()).unwrap())
                .collect();
            assert!(paths == result);
        }
    }
}
