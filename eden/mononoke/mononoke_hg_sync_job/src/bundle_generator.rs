/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use borrowed::borrowed;
use bytes_old::Bytes as BytesOld;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::future;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_01_ext::StreamExt as _;
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
use mercurial_derivation::DeriveHgChangeset;
use mercurial_revlog::RevlogChangeset;
use mercurial_types::HgBlobNode;
use mercurial_types::HgChangesetId;
use mercurial_types::NonRootMPath;
use mononoke_types::datetime::Timestamp;
use mononoke_types::hash::Sha256;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use slog::debug;

use crate::darkstorm_verifier::DarkstormVerifier;
use crate::lfs_verifier::LfsVerifier;
use crate::Repo;

#[async_trait]
pub trait FilterExistingChangesets: Send + Sync {
    async fn filter(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<(ChangesetId, HgChangesetId)>,
    ) -> Result<Vec<(ChangesetId, HgChangesetId)>>;
}

pub async fn create_bundle<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    bookmark: &'a BookmarkKey,
    bookmark_change: &'a BookmarkChange,
    hg_server_heads: Vec<ChangesetId>,
    lfs_params: SessionLfsParams,
    filenode_verifier: &'a FilenodeVerifier,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
    filter_changesets: Arc<dyn FilterExistingChangesets>,
) -> Result<(BytesOld, HashMap<HgChangesetId, (ChangesetId, Timestamp)>)> {
    let mut commits_to_push: Vec<_> = find_commits_to_push(
        ctx,
        repo,
        // Always add "from" bookmark, because is must to be on the hg server
        // If it's not then the push will fail anyway
        hg_server_heads
            .into_iter()
            .chain(bookmark_change.get_from().into_iter()),
        bookmark_change.get_to(),
    )
    .await?
    .try_collect()
    .await?;
    commits_to_push.reverse();
    let commits_to_push = filter_changesets.filter(ctx, commits_to_push).await?;

    debug!(
        ctx.logger(),
        "generating a bundle with {} commits",
        commits_to_push.len()
    );
    let bundle = create_bundle_impl(
        ctx,
        repo,
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
    let timestamps = fetch_timestamps(ctx.clone(), repo.clone(), commits_to_push);
    future::try_join(bundle, timestamps).await
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

    async fn get_from_hg(&self, ctx: &CoreContext, repo: &Repo) -> Result<Option<HgChangesetId>> {
        Self::maybe_get_hg(ctx, self.get_from(), repo).await
    }

    fn get_to(&self) -> Option<ChangesetId> {
        use BookmarkChange::*;

        match self {
            Created(cs_id) => Some(*cs_id),
            Deleted(_) => None,
            Moved { to, .. } => Some(*to),
        }
    }

    async fn get_to_hg(&self, ctx: &CoreContext, repo: &Repo) -> Result<Option<HgChangesetId>> {
        Self::maybe_get_hg(ctx, self.get_to(), repo).await
    }

    async fn maybe_get_hg(
        ctx: &CoreContext,
        maybe_cs: Option<ChangesetId>,
        repo: &Repo,
    ) -> Result<Option<HgChangesetId>> {
        Ok(match maybe_cs {
            Some(cs_id) => Some(repo.derive_hg_changeset(ctx, cs_id).await?),
            None => None,
        })
    }
}

#[derive(Clone)]
pub enum FilenodeVerifier {
    NoopVerifier,
    LfsVerifier(LfsVerifier),
    DarkstormVerifier(DarkstormVerifier),
}

impl FilenodeVerifier {
    async fn verify_entries<'a>(
        &'a self,
        ctx: &'a CoreContext,
        filenode_entries: &'a HashMap<NonRootMPath, Vec<PreparedFilenodeEntry>>,
    ) -> Result<()> {
        let lfs_blobs: Vec<(Sha256, u64)> = filenode_entries
            .values()
            .flat_map(|entries| entries.iter())
            .filter_map(|entry| {
                entry
                    .maybe_get_lfs_pointer()
                    .map(|(sha256, size)| (sha256, size))
            })
            .collect();

        match self {
            Self::NoopVerifier => {}
            Self::LfsVerifier(lfs_verifier) => {
                lfs_verifier.ensure_lfs_presence(ctx, &lfs_blobs).await?;
            }
            // Verification for darkstorm backups - will upload large files bypassing LFS server.
            Self::DarkstormVerifier(ds_verifier) => {
                ds_verifier.upload(ctx, &lfs_blobs).await?;
            }
        }

        Ok(())
    }
}

async fn create_bundle_impl(
    ctx: &CoreContext,
    repo: &Repo,
    bookmark: &BookmarkKey,
    bookmark_change: &BookmarkChange,
    commits_to_push: Vec<HgChangesetId>,
    session_lfs_params: SessionLfsParams,
    filenode_verifier: &FilenodeVerifier,
    push_vars: Option<HashMap<String, bytes::Bytes>>,
) -> Result<BytesOld> {
    let any_commits = !commits_to_push.is_empty();
    let changelog_entries = {
        // These clones are not necessary for futures 0.3 but we need compat for
        // hg methods
        cloned!(ctx);
        let blobstore = repo.repo_blobstore_arc();
        stream::iter(commits_to_push.clone())
            .map(move |hg_cs_id| {
                cloned!(ctx, blobstore);
                async move {
                    let cs = hg_cs_id.load(&ctx, &blobstore).await?;
                    Ok((hg_cs_id, cs))
                }
            })
            .buffered(100)
            .and_then(async move |(hg_cs_id, cs)| {
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
            })
    };

    let entries = get_manifests_and_filenodes(ctx, repo, commits_to_push, &session_lfs_params);

    let ((manifests, prepared_filenode_entries), maybe_from, maybe_to) = future::try_join3(
        entries,
        bookmark_change.get_from_hg(ctx, repo),
        bookmark_change.get_to_hg(ctx, repo),
    )
    .await?;

    let mut bundle2_parts = vec![parts::replycaps_part(create_capabilities())?];

    match push_vars {
        Some(push_vars) if !push_vars.is_empty() => {
            bundle2_parts.push(parts::pushvars_part(push_vars)?)
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

    let filenode_entries = stream::once({
        cloned!(ctx, repo, filenode_verifier);
        async move {
            filenode_verifier
                .verify_entries(&ctx, &prepared_filenode_entries)
                .await?;
            anyhow::Ok(
                create_filenodes(ctx.clone(), repo.clone(), prepared_filenode_entries).compat(),
            )
        }
    })
    .try_flatten()
    .boxed()
    .compat();

    if any_commits {
        bundle2_parts.push(parts::changegroup_part(
            changelog_entries.boxed().compat(),
            Some(filenode_entries.boxify()),
            cg_version,
        )?);

        bundle2_parts.push(parts::treepack_part(
            create_manifest_entries_stream(
                ctx.clone(),
                repo.repo_blobstore().clone(),
                manifests
                    .into_iter()
                    .map(|(path, m_id, cs_id)| (path.into(), m_id, cs_id))
                    .collect(),
            ),
            parts::StoreInHgCache::Yes,
        )?);
    }

    bundle2_parts.push(parts::bookmark_pushkey_part(
        bookmark.to_string(),
        maybe_from.map(|x| x.to_string()).unwrap_or_default(),
        maybe_to.map(|x| x.to_string()).unwrap_or_default(),
    )?);

    let compression = None;
    create_bundle_stream(bundle2_parts, compression)
        .compat()
        .try_concat()
        .await
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

async fn find_commits_to_push<'a>(
    ctx: &'a CoreContext,
    repo: &'a Repo,
    hg_server_heads: impl IntoIterator<Item = ChangesetId>,
    maybe_to_cs_id: Option<ChangesetId>,
) -> Result<impl Stream<Item = Result<(ChangesetId, HgChangesetId)>> + 'a> {
    Ok(repo
        .commit_graph()
        .ancestors_difference_stream(
            ctx,
            maybe_to_cs_id.into_iter().collect(),
            hg_server_heads.into_iter().collect(),
        )
        .await?
        .map_ok(async move |bcs_id| {
            let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;
            Ok((bcs_id, hg_cs_id))
        })
        .try_buffered(100))
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
