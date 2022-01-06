/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bounded_traversal::bounded_traversal_stream;
use futures::{
    future, pin_mut,
    stream::{self, BoxStream},
    FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt,
};
use maplit::hashset;
use std::borrow::Borrow;
use std::collections::{HashSet, VecDeque};

use blobrepo::BlobRepo;
use blobstore::{Blobstore, Loadable};
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use manifest::{Entry, ManifestOps, PathOrPrefix, PathTree};
use mononoke_types::{ChangesetId, DeletedManifestId, FileUnodeId, MPath, ManifestUnodeId};
use repo_derived_data::RepoDerivedDataRef;
use unodes::RootUnodeManifestId;
//use time_ext::DurationExt;

use crate::RootDeletedManifestId;

type UnodeEntry = Entry<ManifestUnodeId, FileUnodeId>;

pub enum PathState {
    // Changeset where the path was deleted and unode where the path was last changed.
    Deleted(Vec<(ChangesetId, UnodeEntry)>),
    // Unode if the path exists.
    Exists(UnodeEntry),
}

/// Find if and when the path deleted.
///
/// Given a changeset and a path returns:
///  * if the paths exists in the commit: the unode corresponding to the path
///  * if it doesn't the unodes and changeset where the path last existed (there might be more than
///    one if deletion happened in separe merge branches)
///  * if the path never existed returns None
///
/// Returns None for deleted files if deleted file manifests are not enabled in a given repo.
///
/// This is the high-level public API of this crate i.e. what clients should use if they want to
/// fetch find where the path was deleted.
pub async fn resolve_path_state(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<PathState>, Error> {
    // if unode exists return entry
    let unode_entry = derive_unode_entry(ctx, repo, cs_id.clone(), path).await?;
    if let Some(unode_entry) = unode_entry {
        return Ok(Some(PathState::Exists(unode_entry)));
    }

    let use_deleted_manifest = repo
        .get_derived_data_config()
        .is_enabled(RootDeletedManifestId::NAME);
    if !use_deleted_manifest {
        return Ok(None);
    }

    // if there is no unode for the commit:path, check deleted manifest
    // the path might be deleted
    stream::try_unfold(
        // starting state
        (VecDeque::from(vec![cs_id.clone()]), hashset! { cs_id }),
        // unfold
        {
            cloned!(ctx, repo, path);
            move |(queue, visited)| {
                resolve_path_state_unfold(ctx.clone(), repo.clone(), path.clone(), queue, visited)
            }
        },
    )
    .map_ok(|deleted_nodes| stream::iter(deleted_nodes).map(Ok::<_, Error>))
    .try_flatten()
    .try_collect::<Vec<_>>()
    .map_ok(move |deleted_nodes| {
        if deleted_nodes.is_empty() {
            None
        } else {
            Some(PathState::Deleted(deleted_nodes))
        }
    })
    .boxed()
    .await
}

async fn resolve_path_state_unfold(
    ctx: CoreContext,
    repo: BlobRepo,
    path: Option<MPath>,
    mut queue: VecDeque<ChangesetId>,
    mut visited: HashSet<ChangesetId>,
) -> Result<
    Option<(
        Vec<(ChangesetId, UnodeEntry)>,
        (VecDeque<ChangesetId>, HashSet<ChangesetId>),
    )>,
    Error,
> {
    let manager = repo.repo_derived_data().manager();
    // let's get deleted manifests for each changeset id
    // and try to find the given path
    if let Some(cs_id) = queue.pop_front() {
        let root_dfm_id = manager
            .derive::<RootDeletedManifestId>(&ctx, cs_id, None)
            .await?;
        let dfm_id = root_dfm_id.deleted_manifest_id();
        let entry = find_entry(&ctx, repo.blobstore(), *dfm_id, path.clone()).await?;

        if let Some(mf_id) = entry {
            // we need to get the linknode, so let's load the deleted manifest
            // if the linknodes is None it means that file should exist
            // but it doesn't, let's throw an error
            let mf = mf_id.load(&ctx, repo.blobstore()).await?;
            let linknode = mf.linknode().ok_or_else(|| {
                let message = format!(
                    "there is no unode for the path '{}' and changeset {:?}, but it exists as a live entry in deleted manifest",
                    MPath::display_opt(path.as_ref()),
                    cs_id,
                );
                Error::msg(message)
            })?;

            // to get last change before deletion we have to look at the liknode
            // parents for the deleted path
            let parents = repo
                .get_changeset_parents_by_bonsai(ctx.clone(), linknode.clone())
                .await?;

            // checking parent unodes
            let parent_unodes = parents.into_iter().map({
                cloned!(ctx, repo, path);
                move |parent| {
                    cloned!(ctx, repo, path);
                    async move {
                        let unode_entry =
                            derive_unode_entry(&ctx, &repo, parent.clone(), &path).await?;
                        Ok::<_, Error>((parent, unode_entry))
                    }
                }
            });
            let parent_unodes = future::try_join_all(parent_unodes).await?;
            return match *parent_unodes {
                [] => {
                    // the linknode must have a parent, otherwise the path couldn't be deleted
                    let message = format!(
                        "the path '{}' was deleted in {:?}, but the changeset doesn't have parents",
                        MPath::display_opt(path.as_ref()),
                        linknode,
                    );
                    Err(Error::msg(message))
                }
                [(_parent, unode_entry)] => {
                    if let Some(unode_entry) = unode_entry {
                        // we've found the last path change before deletion
                        Ok(Some((vec![(linknode, unode_entry)], (queue, visited))))
                    } else {
                        // the unode entry must exist
                        let message = format!(
                            "the path '{}' was deleted in {:?}, but the parent changeset doesn't have a unode",
                            MPath::display_opt(path.as_ref()),
                            linknode,
                        );
                        Err(Error::msg(message))
                    }
                }
                _ => {
                    let mut last_changes = vec![];
                    for (parent, unode_entry) in parent_unodes.into_iter() {
                        if let Some(unode_entry) = unode_entry {
                            // this is one of the last changes
                            last_changes.push((linknode, unode_entry));
                        } else {
                            // the path could have been already deleted here
                            // need to add this node into the queue
                            if visited.insert(parent.clone()) {
                                queue.push_back(parent);
                            }
                        }
                    }
                    Ok(Some((last_changes, (queue, visited))))
                }
            };
        }

        // the path was not deleted here, but could be deleted in other branches
        return Ok(Some((vec![], (queue, visited))));
    }

    Ok(None)
}

async fn derive_unode_entry(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    path: &Option<MPath>,
) -> Result<Option<UnodeEntry>, Error> {
    let root_unode_mf_id = repo
        .repo_derived_data()
        .manager()
        .derive::<RootUnodeManifestId>(ctx, cs_id, None)
        .await?;
    root_unode_mf_id
        .manifest_unode_id()
        .find_entry(ctx.clone(), repo.get_blobstore(), path.clone())
        .await
}

/// List all Deleted Files Manifest paths recursively that were deleted and match specified paths
/// and/or prefixes.
///
pub fn find_entries<'a, I, P>(
    ctx: impl Borrow<CoreContext> + Send + Sync + 'a,
    blobstore: impl Blobstore + 'a,
    manifest_id: DeletedManifestId,
    paths_or_prefixes: I,
) -> impl Stream<Item = Result<(Option<MPath>, DeletedManifestId), Error>> + 'a
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

    async_stream::stream! {
        let ctx = ctx.borrow();
        let blobstore = &blobstore;
        let s: BoxStream<'_, Result<(Option<MPath>, DeletedManifestId), Error>> = bounded_traversal_stream(
            256,
            // starting point
            Some((None, Selector::Selector(path_tree), manifest_id)),
            move |(path, selector, manifest_id)| {
                async move {
                    let mf = manifest_id.load(ctx, blobstore).await?;
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

                            Ok((return_entry, recurse))
                        }
                        Selector::Selector(path_tree) => {
                            let PathTree { value, subentries } = path_tree;

                            match value {
                                Some(Pattern::Prefix) => {
                                    // collect subentries to recurse into
                                    let mut recurse = Vec::new();
                                    for (name, mf_id) in mf.list() {
                                        let next_path =
                                            MPath::join_opt_element(path.as_ref(), &name);
                                        recurse.push((
                                            Some(next_path),
                                            Selector::Recursive,
                                            mf_id.clone(),
                                        ));
                                    }

                                    Ok((return_entry, recurse))
                                }
                                // Rustc bug: 1.50.0 considers the None pattern wrongly unreachable.
                                // https://github.com/rust-lang/rust/issues/82012
                                #[allow(unreachable_patterns)]
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
                                    Result::<_, Error>::Ok((return_path, recurse))
                                }
                            }
                        }
                    }
                }.boxed()
            },
        )
        .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
        .try_flatten()
        .boxed();

        pin_mut!(s);
        while let Some(value) = s.next().await {
            yield value;
        }
    }
}

/// Return Deleted Manifest entry for the given path
///
pub async fn find_entry<'a>(
    ctx: impl Borrow<CoreContext> + Send + Sync + 'a,
    blobstore: impl Blobstore + 'a,
    manifest_id: DeletedManifestId,
    path: Option<MPath>,
) -> Result<Option<DeletedManifestId>, Error> {
    let s = find_entries(ctx, blobstore, manifest_id, vec![PathOrPrefix::Path(path)]);
    pin_mut!(s);
    match s.next().await.transpose()? {
        Some((_path, mf_id)) => Ok(Some(mf_id)),
        None => Ok(None),
    }
}

/// List all Deleted files manifest entries recursively, that represent deleted paths.
///
pub fn list_all_entries<'a>(
    ctx: impl Borrow<CoreContext> + Send + Sync + 'a,
    blobstore: impl Blobstore + 'a,
    manifest_id: DeletedManifestId,
) -> impl Stream<Item = Result<(Option<MPath>, DeletedManifestId), Error>> {
    async_stream::stream! {
        let ctx = ctx.borrow();
        let blobstore = &blobstore;
        let s = bounded_traversal_stream(256, Some((None, manifest_id)), move |(path, manifest_id)| {
            async move {
                let manifest = manifest_id.load(ctx, blobstore).await?;
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

                Result::<_, Error>::Ok((entry, recurse_subentries))
            }.boxed()
        })
        .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
        .try_flatten();

        pin_mut!(s);
        while let Some(value) = s.next().await {
            yield value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::{derive_deleted_files_manifest, get_changes};
    use blobrepo::{save_bonsai_changesets, BlobRepo};
    use fbinit::FacebookInit;
    use fixtures::store_files;
    use manifest::PathOrPrefix;
    use maplit::btreemap;
    use mononoke_types::{
        BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, MPath,
    };
    use repo_derived_data::RepoDerivedDataRef;
    use sorted_vector_map::SortedVectorMap;

    #[fbinit::test]
    async fn test_find_entries(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
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
            let files = store_files(&ctx, file_changes, &repo).await;
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), files, vec![]).await;

            let bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), bcs, vec![]).await;

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
            let files = store_files(&ctx, file_changes, &repo).await;
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), files, vec![bcs_id_1]).await;

            let _bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), bcs, vec![mf_id_1]).await;

            {
                // check that it will yield only two deleted paths
                let mut entries = find_entries(
                    &ctx,
                    repo.blobstore(),
                    mf_id.clone(),
                    vec![
                        PathOrPrefix::Path(Some(path("file.txt"))),
                        PathOrPrefix::Path(Some(path("dir/f-2"))),
                        PathOrPrefix::Path(Some(path("dir/sub/f-1"))),
                    ],
                )
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>()
                .await
                .unwrap();

                entries.sort();
                let expected_entries = vec![Some(path("dir/f-2")), Some(path("dir/sub/f-1"))];
                assert_eq!(entries, expected_entries);
            }

            {
                // check that it will yield recursively all deleted paths including dirs
                let mut entries = find_entries(
                    &ctx,
                    repo.blobstore(),
                    mf_id.clone(),
                    vec![PathOrPrefix::Prefix(Some(path("dir-2")))],
                )
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>()
                .await
                .unwrap();

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
                let mut entries = find_entries(
                    &ctx,
                    repo.blobstore(),
                    mf_id.clone(),
                    vec![
                        PathOrPrefix::Prefix(Some(path("dir/sub"))),
                        PathOrPrefix::Path(Some(path("dir/sub/f-1"))),
                    ],
                )
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>()
                .await
                .unwrap();

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
    async fn test_list_all_entries(fb: FacebookInit) {
        // Test simple separate files and whole dir deletions
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
        let ctx = CoreContext::test_mock(fb);

        // create parent deleted files manifest
        let (bcs_id_1, mf_id_1) = {
            let file_changes = btreemap! {
                "file.txt" => Some("1\n"),
                "dir/sub/f-1" => Some("3\n"),
                "dir/sub/f-3" => Some("3\n"),
                "dir/f-2" => Some("4\n"),
            };
            let files = store_files(&ctx, file_changes, &repo).await;
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), files, vec![]).await;

            let bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), bcs, vec![]).await;

            (bcs_id, mf_id)
        };

        {
            let file_changes = btreemap! {
                "dir/sub/f-1" => None,
                "dir/sub/f-3" => None,
            };
            let files = store_files(&ctx, file_changes, &repo).await;
            let bcs = create_bonsai_changeset(ctx.fb, repo.clone(), files, vec![bcs_id_1]).await;

            let _bcs_id = bcs.get_changeset_id();
            let mf_id = derive_manifest(ctx.clone(), repo.clone(), bcs, vec![mf_id_1]).await;

            {
                // check that it will yield only two deleted paths
                let entries = list_all_entries(ctx.clone(), repo.get_blobstore(), mf_id.clone())
                    .try_collect::<Vec<_>>()
                    .await
                    .unwrap();

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

    async fn derive_manifest(
        ctx: CoreContext,
        repo: BlobRepo,
        bcs: BonsaiChangeset,
        parent_mf_ids: Vec<DeletedManifestId>,
    ) -> DeletedManifestId {
        let bcs_id = bcs.get_changeset_id();

        let changes = get_changes(
            &ctx,
            &repo.repo_derived_data().manager().derivation_context(None),
            bcs,
        )
        .await
        .unwrap();
        derive_deleted_files_manifest(
            &ctx,
            &repo.blobstore().boxed(),
            bcs_id,
            parent_mf_ids,
            changes,
        )
        .await
        .unwrap()
    }

    async fn create_bonsai_changeset(
        fb: FacebookInit,
        repo: BlobRepo,
        file_changes: impl Into<SortedVectorMap<MPath, FileChange>>,
        parents: Vec<ChangesetId>,
    ) -> BonsaiChangeset {
        let bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: Default::default(),
            file_changes: file_changes.into(),
            is_snapshot: false,
        }
        .freeze()
        .unwrap();

        save_bonsai_changesets(vec![bcs.clone()], CoreContext::test_mock(fb), &repo)
            .await
            .unwrap();
        bcs
    }

    fn path(path_str: &str) -> MPath {
        MPath::new(path_str).unwrap()
    }
}
