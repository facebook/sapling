/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::darkstorm_verifier::DarkstormVerifier;
use crate::lfs_verifier::LfsVerifier;
use crate::Repo;
use anyhow::bail;
use anyhow::Error;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use borrowed::borrowed;
use bytes_old::Bytes as BytesOld;
use changeset_fetcher::ChangesetFetcherArc;
use cloned::cloned;
use context::CoreContext;
use futures::compat::Future01CompatExt;
use futures::stream;
use futures::Future as NewFuture;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_01_ext::try_boxfuture;
use futures_01_ext::FutureExt as _;
use futures_01_ext::StreamExt as _;
use futures_old::future::IntoFuture;
use futures_old::stream as stream_old;
use futures_old::Future;
use futures_old::Stream;
use getbundle_response::create_filenodes;
use getbundle_response::create_manifest_entries_stream;
use getbundle_response::get_manifests_and_filenodes;
use getbundle_response::PreparedFilenodeEntry;
use getbundle_response::SessionLfsParams;
use maplit::hashmap;
use mercurial_bundles::capabilities::encode_capabilities;
use mercurial_bundles::capabilities::Capabilities;
use mercurial_bundles::changegroup::CgVersion;
use mercurial_bundles::create_bundle_stream;
use mercurial_bundles::parts;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_revlog::RevlogChangeset;
use mercurial_types::HgBlobNode;
use mercurial_types::HgChangesetId;
use mercurial_types::MPath;
use mononoke_types::datetime::Timestamp;
use mononoke_types::hash::Sha256;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_blobstore::RepoBlobstoreRef;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use slog::debug;
use std::collections::HashMap;
use std::sync::Arc;

pub fn create_bundle(
    ctx: CoreContext,
    repo: Repo,
    lca_hint: Arc<dyn LeastCommonAncestorsHint>,
    bookmark: BookmarkName,
    bookmark_change: BookmarkChange,
    hg_server_heads: Vec<ChangesetId>,
    lfs_params: SessionLfsParams,
    filenode_verifier: FilenodeVerifier,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
) -> impl Future<Item = (BytesOld, HashMap<HgChangesetId, (ChangesetId, Timestamp)>), Error = Error>
{
    let commits_to_push = find_commits_to_push(
        ctx.clone(),
        repo.clone(),
        lca_hint.clone(),
        // Always add "from" bookmark, because is must to be on the hg server
        // If it's not then the push will fail anyway
        hg_server_heads
            .into_iter()
            .chain(bookmark_change.get_from().into_iter()),
        bookmark_change.get_to(),
    )
    .collect()
    .map(|reversed| reversed.into_iter().rev().collect());

    commits_to_push
        .and_then({
            move |commits_to_push: Vec<_>| {
                debug!(
                    ctx.logger(),
                    "generating a bundle with {} commits",
                    commits_to_push.len()
                );
                let bundle = create_bundle_impl(
                    ctx.clone(),
                    repo.clone(),
                    bookmark,
                    bookmark_change,
                    commits_to_push
                        .clone()
                        .into_iter()
                        .map(|(_, hg_cs_id)| hg_cs_id)
                        .collect(),
                    lfs_params,
                    filenode_verifier,
                    push_vars,
                );
                let timestamps = fetch_timestamps(ctx, repo, commits_to_push)
                    .boxed()
                    .compat();
                bundle.join(timestamps)
            }
        })
        .boxify()
}

#[derive(Clone)]
pub enum BookmarkChange {
    Created(ChangesetId),
    Deleted(ChangesetId),
    Moved { from: ChangesetId, to: ChangesetId },
}

impl BookmarkChange {
    pub fn new(
        from_cs_id: Option<ChangesetId>,
        to_cs_id: Option<ChangesetId>,
    ) -> Result<Self, Error> {
        match (from_cs_id, to_cs_id) {
            (Some(ref from), None) => Ok(BookmarkChange::Deleted(*from)),
            (None, Some(ref to)) => Ok(BookmarkChange::Created(*to)),
            (Some(ref from), Some(ref to)) => Ok(BookmarkChange::Moved {
                from: *from,
                to: *to,
            }),
            (None, None) => bail!("unsupported bookmark move: deletion of non-existent bookmark?",),
        }
    }

    fn get_from(&self) -> Option<ChangesetId> {
        use BookmarkChange::*;

        match self {
            Created(_) => None,
            Deleted(cs_id) => Some(*cs_id),
            Moved { from, .. } => Some(*from),
        }
    }

    fn get_from_hg(
        &self,
        ctx: CoreContext,
        repo: &Repo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        Self::maybe_get_hg(ctx, self.get_from(), repo)
    }

    fn get_to(&self) -> Option<ChangesetId> {
        use BookmarkChange::*;

        match self {
            Created(cs_id) => Some(*cs_id),
            Deleted(_) => None,
            Moved { to, .. } => Some(*to),
        }
    }

    fn get_to_hg(
        &self,
        ctx: CoreContext,
        repo: &Repo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        Self::maybe_get_hg(ctx, self.get_to(), repo)
    }

    fn maybe_get_hg(
        ctx: CoreContext,
        maybe_cs: Option<ChangesetId>,
        repo: &Repo,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        cloned!(repo);
        async move {
            let res = match maybe_cs {
                Some(cs_id) => Some(repo.derive_hg_changeset(&ctx, cs_id).await?),
                None => None,
            };
            Ok(res)
        }
        .boxed()
        .compat()
    }
}

#[derive(Clone)]
pub enum FilenodeVerifier {
    NoopVerifier,
    LfsVerifier(LfsVerifier),
    DarkstormVerifier(DarkstormVerifier),
}

impl FilenodeVerifier {
    fn verify_entries(
        &self,
        ctx: CoreContext,
        filenode_entries: &HashMap<MPath, Vec<PreparedFilenodeEntry>>,
    ) -> impl NewFuture<Output = Result<(), Error>> {
        let lfs_blobs: Vec<(Sha256, u64)> = filenode_entries
            .values()
            .flat_map(|entries| entries.iter())
            .filter_map(|entry| {
                entry
                    .maybe_get_lfs_pointer()
                    .map(|(sha256, size)| (sha256, size))
            })
            .collect();

        let this = self.clone();

        async move {
            match this {
                Self::NoopVerifier => {}
                Self::LfsVerifier(lfs_verifier) => {
                    lfs_verifier
                        .ensure_lfs_presence(ctx, &lfs_blobs)
                        .compat()
                        .await?;
                }
                // Verification for darkstorm backups - will upload large files bypassing LFS server.
                Self::DarkstormVerifier(ds_verifier) => {
                    ds_verifier.upload(ctx, &lfs_blobs).await?;
                }
            }

            Ok(())
        }
    }
}

fn create_bundle_impl(
    ctx: CoreContext,
    repo: Repo,
    bookmark: BookmarkName,
    bookmark_change: BookmarkChange,
    commits_to_push: Vec<HgChangesetId>,
    session_lfs_params: SessionLfsParams,
    filenode_verifier: FilenodeVerifier,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
) -> impl Future<Item = BytesOld, Error = Error> {
    let changelog_entries = stream_old::iter_ok(commits_to_push.clone())
        .map({
            cloned!(ctx, repo);
            move |hg_cs_id| {
                cloned!(ctx, repo);
                async move { hg_cs_id.load(&ctx, repo.repo_blobstore()).await }
                    .boxed()
                    .compat()
                    .from_err()
                    .map(move |cs| (hg_cs_id, cs))
            }
        })
        .buffered(100)
        .and_then(|(hg_cs_id, cs)| {
            let revlogcs = RevlogChangeset::new_from_parts(
                cs.parents().clone(),
                cs.manifestid().clone(),
                cs.user().into(),
                cs.time().clone(),
                cs.extra().clone(),
                cs.files().into(),
                cs.message().into(),
            );

            let mut v = Vec::new();
            mercurial_revlog::changeset::serialize_cs(&revlogcs, &mut v)?;
            Ok((
                hg_cs_id.into_nodehash(),
                HgBlobNode::new(bytes::Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
            ))
        });

    let entries = {
        cloned!(ctx, repo, commits_to_push, session_lfs_params);
        async move {
            get_manifests_and_filenodes(&ctx, &repo, commits_to_push, &session_lfs_params).await
        }
        .boxed()
        .compat()
    };

    (
        entries,
        bookmark_change.get_from_hg(ctx.clone(), &repo),
        bookmark_change.get_to_hg(ctx.clone(), &repo),
    )
        .into_future()
        .and_then(
            move |((manifests, prepared_filenode_entries), maybe_from, maybe_to)| {
                let mut bundle2_parts =
                    vec![try_boxfuture!(parts::replycaps_part(create_capabilities()))];

                match push_vars {
                    Some(push_vars) if !push_vars.is_empty() => {
                        bundle2_parts.push(try_boxfuture!(parts::pushvars_part(push_vars)))
                    }
                    _ => {}
                }

                debug!(
                    ctx.logger(),
                    "prepared {} manifests and {} filenodes",
                    manifests.len(),
                    prepared_filenode_entries.len()
                );
                let cg_version = if session_lfs_params.threshold.is_some() {
                    CgVersion::Cg3Version
                } else {
                    CgVersion::Cg2Version
                };

                // Check that the filenodes pass the verifier prior to serializing them.
                let verify_ok = filenode_verifier
                    .verify_entries(ctx.clone(), &prepared_filenode_entries)
                    .boxed()
                    .compat();

                let filenode_entries =
                    create_filenodes(ctx.clone(), repo.clone(), prepared_filenode_entries).boxify();

                let filenode_entries = verify_ok
                    .and_then(move |_| Ok(filenode_entries))
                    .flatten_stream()
                    .boxify();

                if !commits_to_push.is_empty() {
                    bundle2_parts.push(try_boxfuture!(parts::changegroup_part(
                        changelog_entries,
                        Some(filenode_entries),
                        cg_version,
                    )));

                    bundle2_parts.push(try_boxfuture!(parts::treepack_part(
                        create_manifest_entries_stream(
                            ctx,
                            repo.repo_blobstore().clone(),
                            manifests
                        ),
                        parts::StoreInHgCache::Yes
                    )));
                }

                bundle2_parts.push(try_boxfuture!(parts::bookmark_pushkey_part(
                    bookmark.to_string(),
                    maybe_from.map(|x| x.to_string()).unwrap_or_default(),
                    maybe_to.map(|x| x.to_string()).unwrap_or_default(),
                )));

                let compression = None;
                create_bundle_stream(bundle2_parts, compression)
                    .concat2()
                    .boxify()
            },
        )
}

async fn fetch_timestamps(
    ctx: CoreContext,
    repo: Repo,
    hg_cs_ids: impl IntoIterator<Item = (ChangesetId, HgChangesetId)>,
) -> Result<HashMap<HgChangesetId, (ChangesetId, Timestamp)>, Error> {
    async move {
        borrowed!(ctx, repo);
        stream::iter(hg_cs_ids.into_iter().map(Result::<_, Error>::Ok))
            .map(move |res| async move {
                let (cs_id, hg_cs_id) = res?;
                hg_cs_id
                    .load(ctx, repo.repo_blobstore())
                    .err_into()
                    .map_ok(move |hg_blob_cs| (hg_cs_id, (cs_id, hg_blob_cs.time().clone().into())))
                    .await
            })
            .buffered(100)
            .try_collect()
            .await
    }
    .await
}

fn find_commits_to_push(
    ctx: CoreContext,
    repo: Repo,
    lca_hint_index: Arc<dyn LeastCommonAncestorsHint>,
    hg_server_heads: impl IntoIterator<Item = ChangesetId>,
    maybe_to_cs_id: Option<ChangesetId>,
) -> impl Stream<Item = (ChangesetId, HgChangesetId), Error = Error> {
    DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
        ctx.clone(),
        &repo.changeset_fetcher_arc(),
        lca_hint_index,
        maybe_to_cs_id.into_iter().collect(),
        hg_server_heads.into_iter().collect(),
    )
    .map(move |bcs_id| {
        cloned!(ctx, repo);
        async move {
            let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
            Ok((bcs_id, hg_cs_id))
        }
        .boxed()
        .compat()
    })
    .buffered(100)
}

// TODO(stash): this should generate different capabilities depending on whether client
// supports changegroup3 or not
fn create_capabilities() -> BytesOld {
    // List of capabilities that was copied from real bundle generated by Mercurial client.
    let caps_ref = hashmap! {
        "HG20" => vec![],
        "b2x:infinitepush" => vec![],
        "b2x:infinitepushmutation" => vec![],
        "b2x:infinitepushscratchbookmarks" => vec![],
        "b2x:rebase" => vec![],
        "bookmarks" => vec![],
        "changegroup" => vec!["01", "02"],
        "digests" => vec!["md5", "sha1", "sha512"],
        "error" => vec!["abort", "unsupportedcntent", "pushraced", "pushkey"],
        "hgtagsfnodes" => vec![],
        "listkeys" => vec![],
        "phases" => vec!["heads"],
        "pushback" => vec![],
        "pushkey" => vec![],
        "remote-changegroup" => vec!["http", "https"],
        "remotefilelog" => vec!["True"],
        "treemanifest" => vec!["True"],
        "treeonly" => vec!["True"],
    };

    let mut caps = hashmap! {};
    for (key, values) in caps_ref {
        let values = values.into_iter().map(|v| v.to_string()).collect();
        caps.insert(key.to_string(), values);
    }

    encode_capabilities(Capabilities::new(caps))
}
