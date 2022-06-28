/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bounded_traversal::bounded_traversal_stream;
use futures::future;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use maplit::hashset;
use std::borrow::Borrow;
use std::collections::HashSet;
use std::collections::VecDeque;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::PathTree;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use repo_derived_data::RepoDerivedDataRef;
use unodes::RootUnodeManifestId;
//use time_ext::DurationExt;

use crate::mapping::RootDeletedManifestIdCommon;

type UnodeEntry = Entry<ManifestUnodeId, FileUnodeId>;

pub enum PathState {
    // Changeset where the path was deleted and unode where the path was last changed.
    Deleted(Vec<(ChangesetId, UnodeEntry)>),
    // Unode if the path exists.
    Exists(UnodeEntry),
}

#[async_trait::async_trait]
pub trait DeletedManifestOps: RootDeletedManifestIdCommon {
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
    async fn resolve_path_state<'a>(
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        cs_id: ChangesetId,
        path: &'a Option<MPath>,
    ) -> Result<Option<PathState>, Error> {
        // if unode exists return entry
        let unode_entry = derive_unode_entry(ctx, repo, cs_id.clone(), path).await?;
        if let Some(unode_entry) = unode_entry {
            return Ok(Some(PathState::Exists(unode_entry)));
        }

        let use_deleted_manifest = repo.get_derived_data_config().is_enabled(Self::NAME);
        if !use_deleted_manifest {
            return Ok(None);
        }

        // if there is no unode for the commit:path, check deleted manifest
        // the path might be deleted
        let deleted = stream::try_unfold(
            // starting state
            (VecDeque::from(vec![cs_id.clone()]), hashset! { cs_id }),
            // unfold
            {
                cloned!(ctx, repo, path);
                move |(queue, visited)| {
                    Self::resolve_path_state_unfold(
                        ctx.clone(),
                        repo.clone(),
                        path.clone(),
                        queue,
                        visited,
                    )
                }
            },
        )
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if deleted.is_empty() {
            Ok(None)
        } else {
            Ok(Some(PathState::Deleted(deleted)))
        }
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
            let root_dfm_id = manager.derive::<Self>(&ctx, cs_id, None).await?;
            let entry = root_dfm_id
                .find_entry(&ctx, repo.blobstore(), path.clone())
                .await?;

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
                            Ok(Some((vec![(*linknode, unode_entry)], (queue, visited))))
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
                                last_changes.push((*linknode, unode_entry));
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

    /// List all Deleted Manifest paths recursively that were deleted and match specified paths
    /// and/or prefixes.
    ///
    fn find_entries<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        paths_or_prefixes: impl IntoIterator<Item = impl Into<PathOrPrefix>>,
    ) -> BoxStream<'a, Result<(Option<MPath>, Self::Id), Error>> {
        let root_id = self.id().clone();
        enum Pattern {
            Path,
            Prefix,
        }

        enum Selector {
            Selector(PathTree<Option<Pattern>>),
            Recursive,
        }

        let path_tree = PathTree::from_iter(paths_or_prefixes.into_iter().map(|path_or_prefix| {
            match path_or_prefix.into() {
                PathOrPrefix::Path(path) => (path, Some(Pattern::Path)),
                PathOrPrefix::Prefix(prefix) => (prefix, Some(Pattern::Prefix)),
            }
        }));

        (async_stream::stream! {
            let ctx = ctx.borrow();
            let blobstore = &blobstore;
            let s: BoxStream<'_, Result<(Option<MPath>, Self::Id), Error>> = bounded_traversal_stream(
                256,
                // starting point
                Some((None, Selector::Selector(path_tree), root_id)),
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
                                let recurse = mf.into_subentries(ctx, blobstore).map_ok(|(name, mf_id)| {
                                    let next_path = MPath::join_opt_element(path.as_ref(), &name);
                                    (Some(next_path), Selector::Recursive, mf_id)
                                }).try_collect::<Vec<_>>().await?;

                                Ok((return_entry, recurse))
                            }
                            Selector::Selector(path_tree) => {
                                let PathTree { value, subentries } = path_tree;

                                match value {
                                    Some(Pattern::Prefix) => {
                                        // collect subentries to recurse into
                                        let recurse = mf.into_subentries(ctx, blobstore).map_ok(|(name, mf_id)| {
                                            let next_path = MPath::join_opt_element(path.as_ref(), &name);
                                            (Some(next_path), Selector::Recursive, mf_id)
                                        }).try_collect::<Vec<_>>().await?;

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
                                            if let Some(mf_id) = mf.lookup(ctx, blobstore, &name).await? {
                                                let next_path =
                                                    MPath::join_opt_element(path.as_ref(), &name);
                                                recurse.push((
                                                    Some(next_path),
                                                    Selector::Selector(tree),
                                                    mf_id,
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
        }).boxed()
    }

    /// Return Deleted Manifest entry for the given path
    ///
    async fn find_entry(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        path: Option<MPath>,
    ) -> Result<Option<Self::Id>, Error> {
        let s = self.find_entries(ctx, blobstore, vec![PathOrPrefix::Path(path)]);
        pin_mut!(s);
        match s.next().await.transpose()? {
            Some((_path, mf_id)) => Ok(Some(mf_id)),
            None => Ok(None),
        }
    }

    /// List all Deleted manifest entries recursively, that represent deleted paths.
    ///
    fn list_all_entries<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(Option<MPath>, Self::Id), Error>> {
        let root_id = self.id().clone();
        (async_stream::stream! {
            let ctx = ctx.borrow();
            let blobstore = &blobstore;
            let s = bounded_traversal_stream(256, Some((None, root_id)), move |(path, manifest_id)| {
                async move {
                    let manifest = manifest_id.load(ctx, blobstore).await?;
                    let entry = if manifest.is_deleted() {
                        vec![(path.clone(), manifest_id)]
                    } else {
                        vec![]
                    };
                    let recurse_subentries = manifest
                        .into_subentries(ctx, blobstore)
                        .map_ok(|(name, mf_id)| {
                            let full_path = MPath::join_opt_element(path.as_ref(), &name);
                            (Some(full_path), mf_id)
                        })
                        .try_collect::<Vec<_>>().await?;

                    Result::<_, Error>::Ok((entry, recurse_subentries))
                }.boxed()
            })
            .map_ok(|entries| stream::iter(entries.into_iter().map(Ok)))
            .try_flatten();

            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        }).boxed()
    }
}

impl<Root: RootDeletedManifestIdCommon> DeletedManifestOps for Root {}

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
