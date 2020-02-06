/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::{Blobstore, Loadable};
use context::CoreContext;
use futures::{
    stream::{iter_ok, Stream},
    Future,
};
use futures_ext::bounded_traversal::bounded_traversal_stream;
use manifest::{PathOrPrefix, PathTree};
use mononoke_types::{DeletedManifestId, MPath};
use std::iter::FromIterator;

/// List all Deleted Files Manifest paths recursively that were deleted and match specified paths
/// and/or prefixes.
///
pub fn find_entries<I, P>(
    ctx: CoreContext,
    blobstore: impl Blobstore + Clone,
    manifest_id: DeletedManifestId,
    paths_or_prefixes: I,
) -> impl Stream<Item = (Option<MPath>, DeletedManifestId), Error = Error>
where
    I: IntoIterator<Item = P>,
    PathOrPrefix: From<P>,
{
    enum Pattern {
        Path,
        Prefix,
    }

    enum Selector {
        Selector(PathTree<Option<Pattern>>),
        Recursive,
    }

    let path_tree = PathTree::from_iter(paths_or_prefixes.into_iter().map(|path_or_prefix| {
        match PathOrPrefix::from(path_or_prefix) {
            PathOrPrefix::Path(path) => (path, Some(Pattern::Path)),
            PathOrPrefix::Prefix(prefix) => (prefix, Some(Pattern::Prefix)),
        }
    }));

    bounded_traversal_stream(
        256,
        // starting point
        Some((None, Selector::Selector(path_tree), manifest_id)),
        move |(path, selector, manifest_id)| {
            manifest_id.load(ctx.clone(), &blobstore).map(move |mf| {
                let return_entry = if mf.is_deleted() {
                    vec![(path.clone(), manifest_id)]
                } else {
                    vec![]
                };

                match selector {
                    Selector::Recursive => {
                        // collect subentries to recurse into
                        let mut recurse = Vec::new();
                        for (name, mf_id) in mf.list() {
                            let next_path = MPath::join_opt_element(path.as_ref(), &name);
                            recurse.push((Some(next_path), Selector::Recursive, mf_id.clone()));
                        }

                        (return_entry, recurse)
                    }
                    Selector::Selector(path_tree) => {
                        let PathTree { value, subentries } = path_tree;

                        match value {
                            Some(Pattern::Prefix) => {
                                // collect subentries to recurse into
                                let mut recurse = Vec::new();
                                for (name, mf_id) in mf.list() {
                                    let next_path = MPath::join_opt_element(path.as_ref(), &name);
                                    recurse.push((
                                        Some(next_path),
                                        Selector::Recursive,
                                        mf_id.clone(),
                                    ));
                                }

                                (return_entry, recurse)
                            }
                            None | Some(Pattern::Path) => {
                                // need to recurse
                                let mut recurse = vec![];
                                // add path tree selectors
                                for (name, tree) in subentries {
                                    if let Some(mf_id) = mf.lookup(&name) {
                                        let next_path =
                                            MPath::join_opt_element(path.as_ref(), &name);
                                        recurse.push((
                                            Some(next_path),
                                            Selector::Selector(tree),
                                            *mf_id,
                                        ));
                                    }
                                }

                                let return_path = if let Some(Pattern::Path) = value {
                                    return_entry
                                } else {
                                    vec![]
                                };
                                (return_path, recurse)
                            }
                        }
                    }
                }
            })
        },
    )
    .map(|entries| iter_ok(entries))
    .flatten()
}

/// Return Deleted Manifest entry for the given path
///
pub fn find_entry(
    ctx: CoreContext,
    blobstore: impl Blobstore + Clone,
    manifest_id: DeletedManifestId,
    path: Option<MPath>,
) -> impl Future<Item = Option<DeletedManifestId>, Error = Error> {
    find_entries(ctx, blobstore, manifest_id, vec![PathOrPrefix::Path(path)])
        .into_future()
        .then(|result| match result {
            Ok((Some((_path, mf_id)), _stream)) => Ok(Some(mf_id)),
            Ok((None, _stream)) => Ok(None),
            Err((err, _stream)) => Err(err),
        })
}

/// List all Deleted files manifest entries recursively, that represent deleted paths.
///
pub fn list_all_entries(
    ctx: CoreContext,
    blobstore: impl Blobstore + Clone,
    manifest_id: DeletedManifestId,
) -> impl Stream<Item = (Option<MPath>, DeletedManifestId), Error = Error> {
    bounded_traversal_stream(
        256,
        Some((None, manifest_id)),
        move |(path, manifest_id)| {
            manifest_id
                .load(ctx.clone(), &blobstore)
                .map(move |manifest| {
                    let entry = if manifest.is_deleted() {
                        vec![(path.clone(), manifest_id)]
                    } else {
                        vec![]
                    };
                    let recurse_subentries = manifest
                        .list()
                        .map(|(name, mf_id)| {
                            let full_path = MPath::join_opt_element(path.as_ref(), &name);
                            (Some(full_path), mf_id.clone())
                        })
                        .collect::<Vec<_>>();

                    (entry, recurse_subentries)
                })
        },
    )
    .map(|entries| iter_ok(entries))
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::{derive_deleted_files_manifest, get_changes};
    use blobrepo::{save_bonsai_changesets, BlobRepo};
    use blobrepo_factory::new_memblob_empty;
    use fbinit::FacebookInit;
    use fixtures::store_files;
    use manifest::PathOrPrefix;
    use maplit::btreemap;
    use mononoke_types::{
        BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, MPath,
    };
    use std::collections::BTreeMap;
    use tokio::runtime::Runtime;

    #[fbinit::test]
    fn test_find_entries(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo = new_memblob_empty(None).unwrap();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        // create parent deleted files manifest
        let (bcs_id_1, mf_id_1) = {
            let file_changes = btreemap! {
                "file.txt" => Some("1\n"),
                "file-2.txt" => Some("2\n"),
                "dir/sub/f-1" => Some("3\n"),
                "dir/sub/f-6" => Some("3\n"),
                "dir/f-2" => Some("4\n"),
                "dir-2/sub/f-3" => Some("5\n"),
                "dir-2/f-4" => Some("6\n"),
                "dir-2/f-5" => Some("7\n"),
            };
            let bcs = create_bonsai_changeset(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                store_files(ctx.clone(), file_changes, repo.clone()),
                vec![],
            );

            let bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![]);

            (bcs_id, mf_id)
        };

        // delete some files and dirs
        {
            let file_changes = btreemap! {
                "dir/sub/f-1" => None,
                "dir/sub/f-6" => None,
                "dir/f-2" => None,
                "dir-2/sub/f-3" => None,
                "dir-2/f-4" => None,
            };
            let bcs = create_bonsai_changeset(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                store_files(ctx.clone(), file_changes, repo.clone()),
                vec![bcs_id_1],
            );

            let _bcs_id = bcs.get_changeset_id();
            let mf_id =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![mf_id_1]);

            {
                // check that it will yield only two deleted paths
                let f = find_entries(
                    ctx.clone(),
                    repo.get_blobstore(),
                    mf_id.clone(),
                    vec![
                        PathOrPrefix::Path(Some(path("file.txt"))),
                        PathOrPrefix::Path(Some(path("dir/f-2"))),
                        PathOrPrefix::Path(Some(path("dir/sub/f-1"))),
                    ],
                )
                .collect();

                let entries = runtime.block_on(f).unwrap();
                let mut entries = entries
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>();
                entries.sort();
                let expected_entries = vec![Some(path("dir/f-2")), Some(path("dir/sub/f-1"))];
                assert_eq!(entries, expected_entries);
            }

            {
                // check that it will yield recursively all deleted paths including dirs
                let f = find_entries(
                    ctx.clone(),
                    repo.get_blobstore(),
                    mf_id.clone(),
                    vec![PathOrPrefix::Prefix(Some(path("dir-2")))],
                )
                .collect();

                let entries = runtime.block_on(f).unwrap();
                let mut entries = entries
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>();
                entries.sort();
                let expected_entries = vec![
                    Some(path("dir-2/f-4")),
                    Some(path("dir-2/sub")),
                    Some(path("dir-2/sub/f-3")),
                ];
                assert_eq!(entries, expected_entries);
            }

            {
                // check that it will yield recursively even having a path patterns
                let f = find_entries(
                    ctx.clone(),
                    repo.get_blobstore(),
                    mf_id.clone(),
                    vec![
                        PathOrPrefix::Prefix(Some(path("dir/sub"))),
                        PathOrPrefix::Path(Some(path("dir/sub/f-1"))),
                    ],
                )
                .collect();

                let entries = runtime.block_on(f).unwrap();
                let mut entries = entries
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>();
                entries.sort();
                let expected_entries = vec![
                    Some(path("dir/sub")),
                    Some(path("dir/sub/f-1")),
                    Some(path("dir/sub/f-6")),
                ];
                assert_eq!(entries, expected_entries);
            }
        }
    }

    #[fbinit::test]
    fn test_list_all_entries(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo = new_memblob_empty(None).unwrap();
        let mut runtime = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        // create parent deleted files manifest
        let (bcs_id_1, mf_id_1) = {
            let file_changes = btreemap! {
                "file.txt" => Some("1\n"),
                "dir/sub/f-1" => Some("3\n"),
                "dir/sub/f-3" => Some("3\n"),
                "dir/f-2" => Some("4\n"),
            };
            let bcs = create_bonsai_changeset(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                store_files(ctx.clone(), file_changes, repo.clone()),
                vec![],
            );

            let bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![]);

            (bcs_id, mf_id)
        };

        {
            let file_changes = btreemap! {
                "dir/sub/f-1" => None,
                "dir/sub/f-3" => None,
            };
            let bcs = create_bonsai_changeset(
                ctx.fb,
                repo.clone(),
                &mut runtime,
                store_files(ctx.clone(), file_changes, repo.clone()),
                vec![bcs_id_1],
            );

            let _bcs_id = bcs.get_changeset_id();
            let mf_id =
                derive_manifest(ctx.clone(), repo.clone(), &mut runtime, bcs, vec![mf_id_1]);

            {
                // check that it will yield only two deleted paths
                let f =
                    list_all_entries(ctx.clone(), repo.get_blobstore(), mf_id.clone()).collect();

                let entries = runtime.block_on(f).unwrap();
                let mut entries = entries
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>();
                entries.sort();
                let expected_entries = vec![
                    Some(path("dir/sub")),
                    Some(path("dir/sub/f-1")),
                    Some(path("dir/sub/f-3")),
                ];
                assert_eq!(entries, expected_entries);
            }
        }
    }

    fn derive_manifest(
        ctx: CoreContext,
        repo: BlobRepo,
        runtime: &mut Runtime,
        bcs: BonsaiChangeset,
        parent_mf_ids: Vec<DeletedManifestId>,
    ) -> DeletedManifestId {
        let bcs_id = bcs.get_changeset_id();

        let changes = runtime
            .block_on(get_changes(ctx.clone(), repo.clone(), &bcs))
            .unwrap();
        let f = derive_deleted_files_manifest(
            ctx.clone(),
            repo.clone(),
            bcs_id,
            parent_mf_ids,
            changes,
        );

        runtime.block_on(f).unwrap()
    }

    fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        runtime: &mut Runtime,
        file_changes: BTreeMap<MPath, Option<FileChange>>,
        parents: Vec<ChangesetId>,
    ) -> BonsaiChangeset {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: btreemap! {},
            file_changes,
        }
        .freeze()
        .unwrap();

        runtime
            .block_on(save_bonsai_changesets(
                vec![bcs.clone()],
                CoreContext::test_mock(fb),
                repo.clone(),
            ))
            .unwrap();
        bcs
    }

    fn path(path_str: &str) -> MPath {
        MPath::new(path_str).unwrap()
    }
}
