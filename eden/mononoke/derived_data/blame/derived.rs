/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes, Loadable};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use filestore::{self, FetchKey};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{spawn_future, BoxFuture, FutureExt, StreamExt};
use manifest::find_intersection_of_diffs;
use mononoke_types::{
    blame::{store_blame, Blame, BlameId, BlameRejected},
    BonsaiChangeset, ChangesetId, FileUnodeId, MPath,
};
use std::{collections::HashMap, iter::FromIterator, sync::Arc};
use unodes::{find_unode_renames, RootUnodeManifestId, RootUnodeManifestMapping};

pub const BLAME_FILESIZE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct BlameRoot(ChangesetId);

impl BonsaiDerived for BlameRoot {
    const NAME: &'static str = "blame";

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        let csid = bonsai.get_changeset_id();
        let unodes_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));

        let root_manifest =
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unodes_mapping.clone(), csid)
                .map(|mf| mf.manifest_unode_id().clone());
        let parents_manifest = bonsai
            .parents()
            .collect::<Vec<_>>() // iterator should be owned
            .into_iter()
            .map({
                cloned!(ctx, repo);
                move |csid| {
                    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unodes_mapping.clone(), csid)
                        .map(|mf| mf.manifest_unode_id().clone())
                }
            });

        (
            root_manifest,
            future::join_all(parents_manifest),
            find_unode_renames(ctx.clone(), repo.clone(), &bonsai),
        )
            .into_future()
            .and_then(move |(root_mf, parents_mf, renames)| {
                let renames = Arc::new(renames);
                let blobstore = repo.get_blobstore().boxed();
                find_intersection_of_diffs(ctx.clone(), blobstore.clone(), root_mf, parents_mf)
                    .filter_map(|(path, entry)| Some((path?, entry.into_leaf()?)))
                    .map(move |(path, file)| {
                        spawn_future(create_blame(
                            ctx.clone(),
                            blobstore.clone(),
                            renames.clone(),
                            csid,
                            path,
                            file,
                        ))
                    })
                    .buffered(256)
                    .for_each(|_| Ok(()))
                    .map(move |_| BlameRoot(csid))
            })
            .boxify()
    }
}

#[derive(Clone)]
pub struct BlameRootMapping {
    blobstore: Arc<dyn Blobstore>,
}

impl BlameRootMapping {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, csid: &ChangesetId) -> String {
        format!("derived_rootblame.{}", csid)
    }
}

impl BonsaiDerivedMapping for BlameRootMapping {
    type Value = BlameRoot;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let futs = csids.into_iter().map(|csid| {
            self.blobstore
                .get(ctx.clone(), self.format_key(&csid))
                .map(move |val| val.map(|_| (csid.clone(), BlameRoot(csid))))
        });
        stream::FuturesUnordered::from_iter(futs)
            .filter_map(|v| v)
            .collect_to()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, _id: Self::Value) -> BoxFuture<(), Error> {
        self.blobstore.put(
            ctx,
            self.format_key(&csid),
            BlobstoreBytes::from_bytes(Bytes::new()),
        )
    }
}

fn create_blame(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    renames: Arc<HashMap<MPath, FileUnodeId>>,
    csid: ChangesetId,
    path: MPath,
    file_unode_id: FileUnodeId,
) -> impl Future<Item = BlameId, Error = Error> {
    file_unode_id
        .load(ctx.clone(), &blobstore)
        .from_err()
        .and_then(move |file_unode| {
            let parents_content_and_blame: Vec<_> = file_unode
                .parents()
                .iter()
                .cloned()
                .chain(renames.get(&path).cloned())
                .map({
                    cloned!(ctx, blobstore);
                    move |file_unode_id| {
                        (
                            fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id),
                            BlameId::from(file_unode_id)
                                .load(ctx.clone(), &blobstore)
                                .from_err(),
                        )
                            .into_future()
                    }
                })
                .collect();

            (
                fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id),
                future::join_all(parents_content_and_blame),
            )
                .into_future()
                .and_then(move |(content, parents_content)| {
                    let blame_maybe_rejected = match content {
                        Err(rejected) => rejected.into(),
                        Ok(content) => {
                            let parents_content = parents_content
                                .into_iter()
                                .filter_map(|(content, blame_maybe_rejected)| {
                                    Some((content.ok()?, blame_maybe_rejected.into_blame().ok()?))
                                })
                                .collect();
                            Blame::from_parents(csid, content, path, parents_content)?.into()
                        }
                    };
                    Ok(blame_maybe_rejected)
                })
                .and_then(move |blame_maybe_rejected| {
                    store_blame(ctx, &blobstore, file_unode_id, blame_maybe_rejected)
                })
        })
}

pub fn fetch_file_full_content(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    file_unode_id: FileUnodeId,
) -> impl Future<Item = Result<Bytes, BlameRejected>, Error = Error> {
    enum FetchError {
        Rejected(BlameRejected),
        Error(Error),
    }

    fn check_binary(content: &[u8]) -> Result<&[u8], FetchError> {
        if content.contains(&0u8) {
            Err(FetchError::Rejected(BlameRejected::Binary))
        } else {
            Ok(content)
        }
    }

    file_unode_id
        .load(ctx.clone(), &blobstore)
        .map_err(|error| FetchError::Error(error.into()))
        .and_then(move |file_unode| {
            let content_id = *file_unode.content_id();
            filestore::fetch_with_size(&blobstore, ctx, &FetchKey::Canonical(content_id))
                .map_err(FetchError::Error)
                .and_then(move |result| match result {
                    None => {
                        let error =
                            FetchError::Error(format_err!("Missing content: {}", content_id));
                        future::err(error).left_future()
                    }
                    Some((stream, size)) => {
                        if size > BLAME_FILESIZE_LIMIT {
                            return future::err(FetchError::Rejected(BlameRejected::TooBig))
                                .left_future();
                        }
                        stream
                            .map_err(FetchError::Error)
                            .fold(Vec::new(), |mut acc, bytes| {
                                acc.extend(check_binary(bytes.as_ref())?);
                                Ok(acc)
                            })
                            .map(Bytes::from)
                            .right_future()
                    }
                })
        })
        .then(|result| match result {
            Err(FetchError::Error(error)) => Err(error),
            Err(FetchError::Rejected(rejected)) => Ok(Err(rejected)),
            Ok(content) => Ok(Ok(content)),
        })
}
