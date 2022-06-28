/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use futures::future;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use std::collections::HashSet;
use std::sync::Arc;

/// NOTE: To be used only for generating list of files for old, Mercurial format of Changesets.
///
/// This function is used to extract any new files that the given root manifest has provided
/// compared to the provided p1 and p2 parents.
/// A files is considered new when it was not present in neither of parent manifests or it was
/// present, but with a different content.
/// It sorts the returned Vec<MPath> in the order expected by Mercurial.
pub async fn compute_changed_files(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    root: HgManifestId,
    p1: Option<HgManifestId>,
    p2: Option<HgManifestId>,
) -> Result<Vec<MPath>, Error> {
    let files = match (p1, p2) {
        (None, None) => {
            root.list_leaf_entries(ctx, blobstore)
                .map_ok(|(path, _)| path)
                .try_collect()
                .await?
        }
        (Some(manifest), None) | (None, Some(manifest)) => {
            compute_changed_files_pair(ctx, blobstore.clone(), root, manifest).await?
        }
        (Some(p1), Some(p2)) => {
            let changed = future::try_join(
                compute_changed_files_pair(ctx.clone(), blobstore.clone(), root, p1),
                compute_changed_files_pair(ctx.clone(), blobstore.clone(), root, p2),
            )
            .map_ok(|(left, right)| left.intersection(&right).cloned().collect::<Vec<_>>());

            // Mercurial always includes removed files, we need to match this behaviour
            let (ch1, ch2, ch3) = future::try_join3(
                changed,
                compute_removed_files(&ctx, blobstore.clone(), root, Some(p1)),
                compute_removed_files(&ctx, blobstore.clone(), root, Some(p2)),
            )
            .await?;
            ch1.into_iter()
                .chain(ch2.into_iter())
                .chain(ch3.into_iter())
                .collect::<HashSet<_>>()
        }
    };

    let mut files: Vec<MPath> = files.into_iter().collect();
    files.sort_unstable_by(mercurial_mpath_comparator);
    Ok(files)
}

async fn compute_changed_files_pair(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    to: HgManifestId,
    from: HgManifestId,
) -> Result<HashSet<MPath>, Error> {
    from.diff(ctx, blobstore, to)
        .try_filter_map(|diff| async move {
            let (path, entry) = match diff {
                Diff::Added(path, entry) | Diff::Removed(path, entry) => (path, entry),
                Diff::Changed(path, .., entry) => (path, entry),
            };

            match entry {
                Entry::Tree(_) => Ok(None),
                Entry::Leaf(_) => Ok(path),
            }
        })
        .try_collect()
        .await
}

async fn compute_removed_files(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    child: HgManifestId,
    parent: Option<HgManifestId>,
) -> Result<Vec<MPath>, Error> {
    compute_files_with_status(ctx, blobstore, child, parent, move |diff| match diff {
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
    blobstore: Arc<dyn Blobstore>,
    child: HgManifestId,
    parent: Option<HgManifestId>,
    filter_map: impl Fn(Diff<Entry<HgManifestId, (FileType, HgFileNodeId)>>) -> Option<MPath>,
) -> Result<Vec<MPath>, Error> {
    let s = match parent {
        Some(parent) => parent.diff(ctx.clone(), blobstore, child).left_stream(),
        None => child
            .list_all_entries(ctx.clone(), blobstore)
            .map_ok(|(path, entry)| Diff::Added(path, entry))
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
