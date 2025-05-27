/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bytes::Bytes;
use cloned::cloned;
use commit_graph::ArcCommitGraph;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use context::PerfCounterType;
use filenodes_derivation::FilenodesOnlyPublic;
use filestore::FetchKey;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::BufferedParams;
use futures_ext::FbTryStreamExt;
use futures_ext::stream::FbStreamExt;
use futures_stats::TimedTryFutureExt;
use futures_util::try_join;
use manifest::Entry;
use manifest::find_intersection_of_diffs_and_parents;
use mercurial_bundles::changegroup::CgVersion;
use mercurial_bundles::part_encode::PartEncodeBuilder;
use mercurial_bundles::parts;
use mercurial_bundles::parts::FilenodeEntry;
use mercurial_mutation::HgMutationStoreArc;
use mercurial_revlog::RevlogChangeset;
use mercurial_types::FileBytes;
use mercurial_types::HgBlobNode;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgParents;
use mercurial_types::NULL_CSID;
use mercurial_types::NULL_HASH;
use mercurial_types::NonRootMPath;
use mercurial_types::RevFlags;
use mercurial_types::blobs::File;
use mercurial_types::blobs::fetch_manifest_envelope;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::Generation;
use mononoke_types::hash::Sha256;
use mononoke_types::path::MPath;
use phases::Phase;
use phases::Phases;
use phases::PhasesRef;
use rate_limiting::Metric;
use rate_limiting::Scope;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use scuba_ext::FutureStatsScubaExt;
use sha1::Digest;
use sha1::Sha1;
use slog::debug;
use slog::info;

use crate::errors::ErrorKind;

mod errors;

pub const MAX_FILENODE_BYTES_IN_MEMORY: u64 = 100_000_000;
pub const GETBUNDLE_COMMIT_NUM_WARN: u64 = 1_000_000;
const UNEXPECTED_NONE_ERR_MSG: &str = "unexpected None while calling ancestors_difference_stream";

pub trait Repo = CommitGraphArc
    + BonsaiHgMappingRef
    + RepoDerivedDataRef
    + PhasesRef
    + HgMutationStoreArc
    + RepoBlobstoreRef
    + Clone
    + 'static
    + Send
    + Sync;

#[derive(PartialEq, Eq)]
pub enum PhasesPart {
    Yes,
    No,
}

#[derive(Clone)]
pub struct SessionLfsParams {
    pub threshold: Option<u64>,
}

pub async fn create_getbundle_response(
    ctx: &CoreContext,
    repo: &impl Repo,
    common: Vec<HgChangesetId>,
    heads: &[HgChangesetId],
    return_phases: PhasesPart,
    lfs_params: &SessionLfsParams,
) -> Result<Vec<PartEncodeBuilder>, Error> {
    let return_phases = return_phases == PhasesPart::Yes;
    debug!(ctx.logger(), "Return phases is: {:?}", return_phases);

    let heads_len = heads.len();
    let common: HashSet<_> = common.into_iter().collect();

    let phases = repo.phases();
    let (draft_commits, commits_to_send) = try_join!(
        find_new_draft_commits_and_derive_filenodes_for_public_roots(
            ctx, repo, &common, heads, phases
        ),
        find_commits_to_send(ctx, repo, &common, heads),
    )?;

    report_draft_commits(ctx, &draft_commits);

    let mut parts = vec![];
    if heads_len != 0 {
        // no heads means bookmark-only pushrebase, and the client
        // does not expect a changegroup part in this case
        let cg_part =
            create_hg_changeset_part(ctx, repo, commits_to_send.clone(), lfs_params).await?;
        parts.push(cg_part);

        if !draft_commits.is_empty() {
            let mutations_fut = {
                cloned!(ctx);
                let hg_mutation_store = repo.hg_mutation_store_arc();
                async move {
                    hg_mutation_store
                        .all_predecessors(&ctx, draft_commits)
                        .await
                }
            };
            let mut_part = parts::infinitepush_mutation_part(mutations_fut)?;
            parts.push(mut_part);
        }
    }

    // Phases part has to be after the changegroup part.
    if return_phases {
        let phase_heads = find_phase_heads(ctx, repo, heads, phases).await?;
        parts.push(parts::phases_part(
            ctx.clone(),
            stream::iter(phase_heads).map(anyhow::Ok),
        )?);
    }

    Ok(parts)
}

fn report_draft_commits(ctx: &CoreContext, draft_commits: &HashSet<HgChangesetId>) {
    debug!(
        ctx.logger(),
        "Getbundle returning {} draft commits",
        draft_commits.len()
    );
    ctx.perf_counters().add_to_counter(
        PerfCounterType::GetbundleNumDrafts,
        draft_commits.len() as i64,
    );
}

/// return ancestors of heads with hint to exclude ancestors of common
pub async fn find_commits_to_send(
    ctx: &CoreContext,
    repo: &impl Repo,
    common: &HashSet<HgChangesetId>,
    heads: &[HgChangesetId],
) -> Result<Vec<ChangesetId>, Error> {
    let heads = hg_to_bonsai_stream(
        ctx,
        repo,
        heads
            .iter()
            .filter(|head| !common.contains(head))
            .cloned()
            .collect(),
    );

    let excludes = hg_to_bonsai_stream(
        ctx,
        repo,
        common
            .iter()
            .copied()
            .filter(|node| node.into_nodehash() != NULL_CSID.into_nodehash())
            .collect(),
    );

    let (heads, excludes) = try_join!(heads, excludes)?;

    let params = Params { heads, excludes };

    let nodes_to_send: Vec<_> =
        call_difference_of_union_of_ancestors_revset(ctx, &repo.commit_graph_arc(), params, None)
            .await?
            .ok_or_else(|| anyhow!(UNEXPECTED_NONE_ERR_MSG))?
            .into_iter()
            .rev()
            .collect();

    ctx.session()
        .bump_load(Metric::Commits, Scope::Regional, nodes_to_send.len() as f64);
    ctx.perf_counters().add_to_counter(
        PerfCounterType::GetbundleNumCommits,
        nodes_to_send.len() as i64,
    );

    ctx.scuba()
        .clone()
        .log_with_msg("Found commits to send to the client", None);
    Ok(nodes_to_send)
}

#[derive(Default, Clone)]
pub(crate) struct Params {
    heads: Vec<(ChangesetId, Generation)>,
    excludes: Vec<(ChangesetId, Generation)>,
}

impl Params {
    pub fn heads_signature(&self) -> Result<String, Error> {
        Self::signature(&self.heads)
    }

    pub fn excludes_signature(&self) -> Result<String, Error> {
        Self::signature(&self.excludes)
    }

    fn signature(v: &[(ChangesetId, Generation)]) -> Result<String, Error> {
        let mut csids = v.iter().map(|h| h.0).collect::<Vec<_>>();
        csids.sort();
        let mut hasher = Sha1::new();
        for csid in csids {
            hasher.update(csid.blake2().as_ref());
        }
        let res = faster_hex::hex_string(&hasher.finalize());
        Ok(res)
    }
}

async fn call_difference_of_union_of_ancestors_revset(
    ctx: &CoreContext,
    commit_graph: &ArcCommitGraph,
    params: Params,
    limit: Option<u64>,
) -> Result<Option<Vec<ChangesetId>>, Error> {
    let mut scuba = ctx.scuba().clone();
    scuba.add_opt("heads_signature", params.heads_signature().ok());
    scuba.add_opt("excludes_signature", params.excludes_signature().ok());
    scuba.add("heads_count", params.heads.len());
    scuba.add("excludes_count", params.excludes.len());

    let Params { heads, excludes } = params;

    let mut notified_expensive_getbundle = false;
    let min_heads_gen_num = heads.iter().map(|(_, r#gen)| r#gen).min();
    let max_excludes_gen_num = excludes.iter().map(|(_, r#gen)| r#gen).max();
    match (min_heads_gen_num, max_excludes_gen_num) {
        (Some(min_heads), Some(max_excludes)) => {
            if min_heads.difference_from(*max_excludes).unwrap_or(0) > GETBUNDLE_COMMIT_NUM_WARN {
                warn_expensive_getbundle(ctx);
                notified_expensive_getbundle = true;
            }
        }
        _ => {}
    };

    let nodes_to_send = commit_graph
        .ancestors_difference_stream(
            ctx,
            heads.into_iter().map(|(cs_id, _gen)| cs_id).collect(),
            excludes.into_iter().map(|(cs_id, _gen)| cs_id).collect(),
        )
        .await?
        .yield_periodically()
        .inspect({
            let mut i = 0;
            move |_| {
                i += 1;
                if i > GETBUNDLE_COMMIT_NUM_WARN && !notified_expensive_getbundle {
                    notified_expensive_getbundle = true;
                    warn_expensive_getbundle(ctx);
                }
            }
        });

    let res = async move {
        if let Some(limit) = limit {
            let res: Vec<_> = nodes_to_send
                .take(limit.try_into().unwrap())
                .try_collect()
                .await?;
            if res.len() as u64 == limit {
                Ok(None)
            } else {
                Ok(Some(res))
            }
        } else {
            nodes_to_send.try_collect().map_ok(Some).await
        }
    }
    .try_timed()
    .await?
    .log_future_stats(scuba, "call_difference_of_union_of_ancestors_revset", None);

    Ok(res)
}

fn warn_expensive_getbundle(ctx: &CoreContext) {
    info!(
        ctx.logger(),
        "your repository is out of date and pulling new commits might take a long time. \
        Please consider recloning your repository since it might be much faster."
        ; "remote" => "true"
    );
}

async fn create_hg_changeset_part(
    ctx: &CoreContext,
    repo: &impl Repo,
    nodes_to_send: Vec<ChangesetId>,
    lfs_params: &SessionLfsParams,
) -> Result<PartEncodeBuilder> {
    let map_chunk_size = 100;
    let load_buffer_size = 1000;

    let changelogentries = stream::iter(nodes_to_send)
        .chunks(map_chunk_size)
        .then({
            cloned!(ctx, repo);
            move |bonsais| {
                cloned!(ctx, repo);
                async move {
                    let mapping = repo
                        .get_hg_bonsai_mapping(ctx.clone(), bonsais.clone())
                        .await?
                        .into_iter()
                        .map(|(hg_cs_id, bonsai_cs_id)| (bonsai_cs_id, hg_cs_id))
                        .collect::<HashMap<_, _>>();

                    // We need to preserve ordering of the Bonsais for Mercurial on the client-side.

                    let ordered_mapping = bonsais
                        .into_iter()
                        .map(|bcs_id| {
                            let hg_cs_id = mapping.get(&bcs_id).ok_or_else(|| {
                                anyhow::format_err!("cs_id was missing from mapping: {:?}", bcs_id)
                            })?;
                            Ok((*hg_cs_id, bcs_id))
                        })
                        .collect::<Vec<_>>();

                    Result::<_, Error>::Ok(ordered_mapping)
                }
            }
        })
        .map_ok(stream::iter)
        .try_flatten()
        .map({
            cloned!(ctx, repo);
            move |res| {
                cloned!(ctx, repo);
                async move {
                    match res {
                        Ok((hg_cs_id, _bcs_id)) => {
                            let cs = hg_cs_id.load(&ctx, repo.repo_blobstore()).await?;
                            Ok((hg_cs_id, cs))
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        })
        .buffered(load_buffer_size)
        .and_then(|(hg_cs_id, cs)| async move {
            let node = hg_cs_id.into_nodehash();

            let revlogcs = RevlogChangeset::new_from_parts(
                cs.parents(),
                cs.manifestid(),
                Bytes::copy_from_slice(cs.user()),
                cs.time().clone(),
                cs.extra().clone(),
                cs.files().into(),
                Bytes::copy_from_slice(cs.message()),
            );

            let mut v = Vec::new();
            mercurial_revlog::changeset::serialize_cs(&revlogcs, &mut v)?;

            Ok((
                node,
                HgBlobNode::new(Bytes::from(v), revlogcs.p1(), revlogcs.p2()),
            ))
        });

    let cg_version = if lfs_params.threshold.is_some() {
        CgVersion::Cg3Version
    } else {
        CgVersion::Cg2Version
    };

    parts::changegroup_part(changelogentries, None, cg_version)
}

async fn hg_to_bonsai_stream(
    ctx: &CoreContext,
    repo: &impl Repo,
    nodes: Vec<HgChangesetId>,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    stream::iter(nodes)
        .map({
            move |node| async move {
                let bcs_id = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, node)
                    .await?
                    .ok_or(ErrorKind::BonsaiNotFoundForHgChangeset(node))?;

                let gen_num = repo
                    .commit_graph()
                    .changeset_generation(ctx, bcs_id)
                    .await?;
                Ok((bcs_id, gen_num))
            }
        })
        .buffered(100)
        .try_collect()
        .await
}

// Given a set of heads and a set of common heads, find the new draft commits,
// and ensure all the public heads and first public ancestors of the draft commits
// have had their filenodes derived.
pub async fn find_new_draft_commits_and_derive_filenodes_for_public_roots(
    ctx: &CoreContext,
    repo: &impl Repo,
    common: &HashSet<HgChangesetId>,
    heads: &[HgChangesetId],
    phases: &dyn Phases,
) -> Result<HashSet<HgChangesetId>, Error> {
    let (draft, public_heads) =
        find_new_draft_commits_and_public_roots(ctx, repo, common, heads, phases).await?;

    ctx.scuba().clone().log_with_msg("Deriving filenodes", None);
    // Ensure filenodes are derived for all of the public heads.
    stream::iter(public_heads)
        .map(|bcs_id| {
            repo.repo_derived_data()
                .derive::<FilenodesOnlyPublic>(ctx, bcs_id)
        })
        .buffered(100)
        .try_for_each(|_derive| async { Ok(()) })
        .await?;
    ctx.scuba()
        .clone()
        .log_with_msg("Derived all filenodes", None);

    Ok(draft)
}

/// Given a set of heads and set of common heads, find the new draft commits,
/// that is, draft commits that are ancestors of the heads but not ancestors of
/// the common heads, as well as the new public heads and the first public
/// ancestors of the new draft commits.
///
/// The draft commits are returned as `HgChangesetId`; the public heads are
/// returned as `ChangesetId`.
///
/// For example in the graph:
/// ```ignore
///   o F [public]
///   |
///   | o E [draft]
///   | |
///   | | o D [draft]
///   | | |
///   | | o C [draft]
///   | |/
///   | o B [public]
///   |/
///   o A [public]
/// ```
///
/// If `heads = [D, E, F]` and `common = [C]` then `new_draft_commits = [D, E]`,
/// and new_public_heads = `[A, B]`.
async fn find_new_draft_commits_and_public_roots(
    ctx: &CoreContext,
    repo: &impl Repo,
    common: &HashSet<HgChangesetId>,
    heads: &[HgChangesetId],
    phases: &dyn Phases,
) -> Result<(HashSet<HgChangesetId>, HashSet<ChangesetId>), Error> {
    // Remove the common heads.
    let new_heads: Vec<_> = heads
        .iter()
        .filter(|hg_cs_id| !common.contains(hg_cs_id))
        .cloned()
        .collect();

    // Traverse the draft commits, accumulating all of the draft commits and the
    // public heads encountered.
    let mut new_hg_draft_commits = HashSet::new();
    let mut new_public_heads = HashSet::new();
    traverse_draft_commits(
        ctx,
        repo,
        phases,
        &new_heads,
        |public_bcs_id, _public_hg_cs_id| {
            new_public_heads.insert(public_bcs_id);
        },
        |_draft_head_bcs_id, _draft_head_hg_cs_id| {},
        |_draft_bcs_id, draft_hg_cs_id| {
            // If we encounter a common head, stop traversing there.  Ideally
            // we would stop if we encountered any ancestor of a common head,
            // however the common set may be large, and comparing ancestors
            // against all of these would be too expensive.
            let traverse = !common.contains(&draft_hg_cs_id);
            if traverse {
                new_hg_draft_commits.insert(draft_hg_cs_id);
            }
            traverse
        },
    )
    .await?;

    Ok((new_hg_draft_commits, new_public_heads))
}

/// Return phase heads for all public and draft heads, and the public roots of
/// the draft heads, that is, the first public ancestor of the draft heads.
///
/// For example in the graph:
/// ```ignore
///   o F [public]
///   |
///   | o E [draft]
///   | |
///   | | o D [draft]
///   | | |
///   | | o C [draft]
///   | |/
///   | o B [public]
///   |/
///   o A [public]
/// ```
///
/// If `heads = [D, E, F]` then this will return
/// `[(F, public), (E, draft), (D, draft), (B, public)]`
async fn find_phase_heads(
    ctx: &CoreContext,
    repo: &impl Repo,
    heads: &[HgChangesetId],
    phases: &dyn Phases,
) -> Result<Vec<(HgChangesetId, Phase)>, Error> {
    // Traverse the draft commits, collecting phase heads for the draft heads
    // and public commits that we encounter.
    let mut phase_heads = Vec::new();
    let mut draft_heads = Vec::new();
    traverse_draft_commits(
        ctx,
        repo,
        phases,
        heads,
        |_public_bcs_id, public_hg_cs_id| {
            phase_heads.push((public_hg_cs_id, Phase::Public));
        },
        |_draft_head_bcs_id, draft_head_hg_cs_id| {
            draft_heads.push((draft_head_hg_cs_id, Phase::Draft));
        },
        |_draft_bcs_id, _draft_hg_cs_id| true,
    )
    .await?;
    phase_heads.append(&mut draft_heads);
    Ok(phase_heads)
}

/// Traverses all draft commits, calling `draft_head_callback` on each draft
/// head encountered, `draft_callback` on each draft commit encountered
/// (including heads) and `public_callback` on the first public commit
/// encountered. The parents of draft commits are only traversed if
/// `draft_callback` returns true.
async fn traverse_draft_commits(
    ctx: &CoreContext,
    repo: &(impl CommitGraphRef + RepoDerivedDataRef + BonsaiHgMappingRef + Send + Sync),
    phases: &dyn Phases,
    heads: &[HgChangesetId],
    mut public_callback: impl FnMut(ChangesetId, HgChangesetId),
    mut draft_head_callback: impl FnMut(ChangesetId, HgChangesetId),
    mut draft_callback: impl FnMut(ChangesetId, HgChangesetId) -> bool,
) -> Result<(), Error> {
    // Find the bonsai changeset id for all of the heads.
    let hg_bonsai_heads = repo
        .get_hg_bonsai_mapping(ctx.clone(), heads.to_vec())
        .await?;

    // Find the initial set of public changesets.
    let bonsai_heads = hg_bonsai_heads.iter().map(|(_, bcs_id)| *bcs_id).collect();
    let mut public_changesets = phases.get_cached_public(ctx, bonsai_heads).await?;

    // Call the draft head callback for each of the draft heads.
    let mut seen = HashSet::new();
    let mut next_changesets = Vec::new();
    for (hg_cs_id, bcs_id) in hg_bonsai_heads.into_iter() {
        if !public_changesets.contains(&bcs_id) {
            draft_head_callback(bcs_id, hg_cs_id);
        }
        next_changesets.push((hg_cs_id, bcs_id));
        seen.insert(bcs_id);
    }

    while !next_changesets.is_empty() {
        let mut traverse = Vec::new();
        for (hg_cs_id, bcs_id) in next_changesets {
            if public_changesets.contains(&bcs_id) {
                public_callback(bcs_id, hg_cs_id);
            } else if draft_callback(bcs_id, hg_cs_id) {
                traverse.push(bcs_id);
            }
        }

        if traverse.is_empty() {
            break;
        }

        // Get the parents of the changesets we are traversing.
        let parents: Vec<_> = stream::iter(traverse)
            .map(move |csid| async move {
                let parents = repo.commit_graph().changeset_parents(ctx, csid).await?;

                Result::<_, Error>::Ok(parents)
            })
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .filter(|csid| seen.insert(*csid))
            .collect();

        let (new_next_changesets, new_public_changesets) = try_join!(
            repo.get_hg_bonsai_mapping(ctx.clone(), parents.clone()),
            phases.get_cached_public(ctx, parents)
        )?;
        next_changesets = new_next_changesets;
        public_changesets = new_public_changesets;
    }

    Ok(())
}

pub enum FilenodeEntryContent {
    InlineV2(ContentId),
    InlineV3(ContentId),
    LfsV3(Sha256, u64),
}

pub struct PreparedFilenodeEntry {
    pub filenode: HgFileNodeId,
    pub linknode: HgChangesetId,
    pub parents: HgParents,
    pub metadata: Bytes,
    pub content: FilenodeEntryContent,
    /// This field represents the memory footprint of a single
    /// entry when streaming. For inline-stored entries, this is
    /// just the size of the contents, while for LFS this is a size
    /// of an LFS pointer. Of course, this does not have to be
    /// precise, as it just provides an estimate on when to stop
    /// buffering.
    pub entry_weight_hint: u64,
}

impl PreparedFilenodeEntry {
    async fn into_filenode<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a RepoBlobstore,
    ) -> Result<(HgFileNodeId, HgChangesetId, HgBlobNode, Option<RevFlags>), Error> {
        let Self {
            filenode,
            linknode,
            parents,
            metadata,
            content,
            ..
        } = self;

        async fn fetch_and_wrap(
            ctx: &CoreContext,
            blobstore: &RepoBlobstore,
            content_id: ContentId,
        ) -> Result<FileBytes, Error> {
            let content = filestore::fetch_concat(blobstore, ctx, content_id).await?;

            Ok(FileBytes(content))
        }

        let (blob, flags) = match content {
            FilenodeEntryContent::InlineV2(content_id) => {
                let bytes = fetch_and_wrap(ctx, blobstore, content_id).await?;
                (generate_inline_file(&bytes, parents, &metadata), None)
            }
            FilenodeEntryContent::InlineV3(content_id) => {
                let bytes = fetch_and_wrap(ctx, blobstore, content_id).await?;
                (
                    generate_inline_file(&bytes, parents, &metadata),
                    Some(RevFlags::REVIDX_DEFAULT_FLAGS),
                )
            }
            FilenodeEntryContent::LfsV3(oid, size) => (
                generate_lfs_file(oid, parents, size, &metadata)?,
                Some(RevFlags::REVIDX_EXTSTORED),
            ),
        };

        Ok((filenode, linknode, blob, flags))
    }

    pub fn maybe_get_lfs_pointer(&self) -> Option<(Sha256, u64)> {
        match self.content {
            FilenodeEntryContent::LfsV3(sha256, size) => Some((sha256.clone(), size)),
            _ => None,
        }
    }
}

fn calculate_content_weight_hint(content_size: u64, content: &FilenodeEntryContent) -> u64 {
    match content {
        // Approximate calculation for LFS:
        // - 34 bytes for GitVersion
        // - 32 bytes for Sha256
        // - 8 bytes for size
        // - 40 bytes for parents
        FilenodeEntryContent::LfsV3(_, _) => 34 + 32 + 8 + 40,
        // Approximate calculation for inline:
        // - content_size
        // - parents
        FilenodeEntryContent::InlineV2(_) | FilenodeEntryContent::InlineV3(_) => content_size + 40,
    }
}

fn prepare_filenode_entries_stream<'a>(
    ctx: &'a CoreContext,
    blobstore: &'a RepoBlobstore,
    filenodes: Vec<(NonRootMPath, HgFileNodeId, HgChangesetId)>,
    lfs_session: &'a SessionLfsParams,
) -> impl Stream<Item = Result<(NonRootMPath, Vec<PreparedFilenodeEntry>), Error>> + 'a {
    stream::iter(filenodes)
        .map({
            move |(path, filenode, linknode)| async move {
                let envelope = filenode.load(ctx, blobstore).await?;

                let file_size = envelope.content_size();

                let content = match lfs_session.threshold {
                    None => FilenodeEntryContent::InlineV2(envelope.content_id()),
                    Some(lfs_threshold) if file_size <= lfs_threshold => {
                        FilenodeEntryContent::InlineV3(envelope.content_id())
                    }
                    _ => {
                        let key = FetchKey::from(envelope.content_id());
                        let meta = filestore::get_metadata(blobstore, ctx, &key).await?;
                        let meta =
                            meta.ok_or_else(|| Error::from(ErrorKind::MissingContent(key)))?;
                        let oid = meta.sha256;
                        FilenodeEntryContent::LfsV3(oid, file_size)
                    }
                };

                let parents = envelope.hg_parents();
                let entry_weight_hint = calculate_content_weight_hint(file_size, &content);
                let prepared_filenode_entry = PreparedFilenodeEntry {
                    filenode,
                    linknode,
                    parents,
                    metadata: envelope.metadata().clone(),
                    content,
                    entry_weight_hint,
                };

                Ok((path, vec![prepared_filenode_entry]))
            }
        })
        .buffered(100)
}

fn generate_inline_file(content: &FileBytes, parents: HgParents, metadata: &Bytes) -> HgBlobNode {
    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();

    // Metadata is only used to store copy/rename information
    let no_rename_metadata = metadata.is_empty();
    let mut res = vec![];
    res.extend(metadata);
    res.extend(content.as_bytes());
    if no_rename_metadata {
        HgBlobNode::new(Bytes::from(res), p1, p2)
    } else {
        // Mercurial has a complicated logic regarding storing renames
        // If copy/rename metadata is stored then p1 is always "null"
        // (i.e. hash like "00000000....") - that's why we set it to None below.
        // p2 is null for a non-merge commit, but not-null for merges.
        // (See D6922881 for more details about merge logic)
        //
        // It boils down to the fact that we can't have both p1 and p2 to be
        // non-null if we have rename metadata.
        // `HgFileEnvelope::hg_parents()` returns HgParents structure, which
        // always makes p2 a null commit if at least one parent commit is null.
        // And that's why we set the second parent to p1 below.
        debug_assert!(p2.is_none());
        HgBlobNode::new(Bytes::from(res), None, p1)
    }
}

fn generate_lfs_file(
    oid: Sha256,
    parents: HgParents,
    file_size: u64,
    metadata: &Bytes,
) -> Result<HgBlobNode, Error> {
    let copy_from = File::extract_copied_from(metadata)?;
    let bytes = File::generate_lfs_file(oid, file_size, copy_from)?;

    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();
    Ok(HgBlobNode::new(bytes, p1, p2))
}

pub fn create_manifest_entries_stream(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    manifests: Vec<(MPath, HgManifestId, HgChangesetId)>,
) -> BoxStream<'static, Result<BoxFuture<'static, Result<parts::TreepackPartInput, Error>>, Error>>
{
    stream::iter(
        manifests
            .into_iter()
            .filter(|(fullpath, mf_id, _linknode)| {
                !(fullpath.is_root() && mf_id.clone().into_nodehash() == NULL_HASH)
            }),
    )
    .map({
        move |(fullpath, mf_id, linknode)| {
            cloned!(ctx, blobstore);
            Ok(async move {
                let mf_envelope = fetch_manifest_envelope(&ctx, &blobstore.boxed(), mf_id).await?;
                let (p1, p2) = mf_envelope.parents();
                Ok(parts::TreepackPartInput {
                    node: mf_id.into_nodehash(),
                    p1,
                    p2,
                    content: mf_envelope.contents().clone(),
                    fullpath,
                    linknode: linknode.into_nodehash(),
                })
            }
            .boxed())
        }
    })
    .boxed()
}

async fn diff_with_parents(
    ctx: &CoreContext,
    repo: &(
         impl CommitGraphRef + RepoDerivedDataRef + BonsaiHgMappingRef + RepoBlobstoreRef + Send + Sync
     ),
    hg_cs_id: HgChangesetId,
) -> Result<
    (
        Vec<(MPath, HgManifestId, HgChangesetId)>,
        Vec<(NonRootMPath, HgFileNodeId, HgChangesetId)>,
    ),
    Error,
> {
    let (mf_id, parent_mf_ids) = try_join!(
        fetch_manifest(ctx, repo.repo_blobstore(), &hg_cs_id),
        async {
            let parents = repo.get_hg_changeset_parents(ctx.clone(), hg_cs_id).await?;

            future::try_join_all(
                parents
                    .iter()
                    .map(|p| fetch_manifest(ctx, repo.repo_blobstore(), p)),
            )
            .await
        }
    )?;

    let blobstore = Arc::new(repo.repo_blobstore().clone());
    let new_entries: Vec<(MPath, Entry<_, _>, _)> =
        find_intersection_of_diffs_and_parents(ctx.clone(), blobstore, mf_id, parent_mf_ids)
            .try_collect()
            .await?;

    let mut mfs = vec![];
    let mut files = vec![];
    for (path, entry, parent_entries) in new_entries {
        match entry {
            Entry::Tree(mf) => {
                mfs.push((path, mf, hg_cs_id.clone()));
            }
            Entry::Leaf((_, file)) => {
                let mut found_same_in_parents = false;
                for p in parent_entries {
                    if let Entry::Leaf((_, parent_file)) = p {
                        if parent_file == file {
                            found_same_in_parents = true;
                            break;
                        }
                    }
                }
                if found_same_in_parents {
                    continue;
                }
                let path = Option::<NonRootMPath>::from(path).expect("empty file paths?");
                files.push((path, file, hg_cs_id.clone()));
            }
        }
    }

    Ok((mfs, files))
}

fn create_filenodes_weighted(
    ctx: CoreContext,
    repo: impl RepoBlobstoreRef + Clone + Sync + Send + 'static,
    entries: HashMap<NonRootMPath, Vec<PreparedFilenodeEntry>>,
) -> impl Stream<
    Item = Result<
        (
            impl Future<Output = Result<(NonRootMPath, Vec<FilenodeEntry>), Error>>,
            u64,
        ),
        Error,
    >,
> {
    let items = entries.into_iter().map({
        cloned!(ctx, repo);
        move |(path, prepared_entries)| {
            let total_weight: u64 = prepared_entries.iter().fold(0, |acc, prepared_entry| {
                acc + prepared_entry.entry_weight_hint
            });

            let entry_futs: Vec<_> = prepared_entries
                .into_iter()
                .map({
                    |entry| {
                        cloned!(ctx, repo);
                        async move { entry.into_filenode(&ctx, repo.repo_blobstore()).await }
                    }
                })
                .collect();

            let fut = future::try_join_all(entry_futs).map_ok(|entries| (path, entries));

            anyhow::Ok((fut, total_weight))
        }
    });
    stream::iter(items)
}

pub fn create_filenodes(
    ctx: CoreContext,
    repo: impl RepoBlobstoreRef + Clone + Sync + Send + 'static,
    entries: HashMap<NonRootMPath, Vec<PreparedFilenodeEntry>>,
) -> impl Stream<Item = Result<(NonRootMPath, Vec<FilenodeEntry>), Error>> {
    let params = BufferedParams {
        weight_limit: MAX_FILENODE_BYTES_IN_MEMORY,
        buffer_size: 100,
    };
    create_filenodes_weighted(ctx, repo, entries).try_buffered_weight_limited(params)
}

// This function preserves the topological order of entries i.e. filenods or manifest
// created in an earlier commit will be earlier in the output.
pub async fn get_manifests_and_filenodes(
    ctx: &CoreContext,
    repo: &(
         impl CommitGraphRef + RepoDerivedDataRef + BonsaiHgMappingRef + RepoBlobstoreRef + Send + Sync
     ),
    commits: impl IntoIterator<Item = HgChangesetId>,
    lfs_params: &SessionLfsParams,
) -> Result<
    (
        Vec<(MPath, HgManifestId, HgChangesetId)>,
        HashMap<NonRootMPath, Vec<PreparedFilenodeEntry>>,
    ),
    Error,
> {
    let entries: Vec<_> = stream::iter(commits)
        .then({
            |hg_cs_id| async move {
                let (manifests, filenodes) = diff_with_parents(ctx, repo, hg_cs_id).await?;

                let filenodes: Vec<(NonRootMPath, Vec<PreparedFilenodeEntry>)> =
                    prepare_filenode_entries_stream(
                        ctx,
                        repo.repo_blobstore(),
                        filenodes,
                        lfs_params,
                    )
                    .try_collect()
                    .await?;
                Result::<_, Error>::Ok((manifests, filenodes))
            }
        })
        .try_collect()
        .await?;

    // We avoid duplicate manifests and filenodes, but we preserve the order of the entries
    // with respect to the commit i.e. entries from earlier commit are before entries
    // from the later commit.
    let mut all_mf_entries = vec![];
    let mut used_mfs = HashSet::new();

    let mut ordered_filenode_entries: HashMap<_, Vec<_>> = HashMap::new();
    let mut used_filenodes: HashMap<NonRootMPath, HashSet<HgFileNodeId>> = HashMap::new();
    for (mf_entries, file_entries) in entries {
        for (path, mf_id, linknode) in mf_entries {
            if used_mfs.insert((path.clone(), mf_id)) {
                all_mf_entries.push((path, mf_id, linknode));
            }
        }

        for (file_path, filenodes) in file_entries {
            let used_filenodes = used_filenodes.entry(file_path.clone()).or_default();
            let ordered_entries = ordered_filenode_entries.entry(file_path).or_default();

            for filenode in filenodes {
                if used_filenodes.insert(filenode.filenode) {
                    ordered_entries.push(filenode);
                }
            }
        }
    }

    Ok((all_mf_entries, ordered_filenode_entries))
}

async fn fetch_manifest(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    hg_cs_id: &HgChangesetId,
) -> Result<HgManifestId, Error> {
    let blob_cs = hg_cs_id.load(ctx, blobstore).await?;
    Ok(blob_cs.manifestid())
}
