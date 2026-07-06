/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use acl_manifest::RootAclManifestId;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use cacheblob::MemWritesKeyedBlobstore;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream::BoxStream;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_derivation::derive_hg_augmented_manifest::cached_acl_overlay_map;
use mercurial_derivation::derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai_changeset;
use mercurial_derivation::derive_hg_augmented_manifest::normalize_acl_root;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::sharded_augmented_manifest::HgAugmentedManifestEntry;
use mercurial_types::sharded_augmented_manifest::HgAugmentedManifestEnvelope;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::typed_hash::AclManifestId;
use repo_blobstore::RepoBlobstoreRef;
use restricted_paths_common::ArcRestrictedPathsConfigBased;
use restricted_paths_common::NoopRestrictedPathsManifestIdStore;
use restricted_paths_common::RestrictedPathsConfigBased;
use tracing::info;

use super::Repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifyMismatch {
    pub(crate) cs_id: ChangesetId,
    pub(crate) field: String,
    pub(crate) computed: String,
    pub(crate) expected: String,
}

impl VerifyMismatch {
    fn new(
        cs_id: ChangesetId,
        field: impl Into<String>,
        computed: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            cs_id,
            field: field.into(),
            computed: computed.into(),
            expected: expected.into(),
        }
    }
}

pub(crate) struct Verifier<'a> {
    ctx: &'a CoreContext,
    repo: &'a Repo,
    manager: &'a DerivedDataManager,
    acl_cache: HashMap<AclManifestId, Arc<HashMap<MPath, AclManifestId>>>,
    restricted_paths: ArcRestrictedPathsConfigBased,
}

impl<'a> Verifier<'a> {
    pub(crate) fn new(
        ctx: &'a CoreContext,
        repo: &'a Repo,
        manager: &'a DerivedDataManager,
    ) -> Self {
        let derivation_ctx = manager.derivation_context(None);
        let real_restricted_paths = derivation_ctx.restricted_paths();
        // Verification must not write restricted-path manifest ids while re-deriving.
        let restricted_paths = Arc::new(RestrictedPathsConfigBased::new(
            real_restricted_paths.config().clone(),
            Arc::new(NoopRestrictedPathsManifestIdStore::new(
                derivation_ctx.repo_id(),
            )),
            None,
        ));

        Self {
            ctx,
            repo,
            manager,
            acl_cache: HashMap::new(),
            restricted_paths,
        }
    }

    pub(crate) async fn verify_one(
        &mut self,
        cs_id: ChangesetId,
    ) -> Result<Option<VerifyMismatch>> {
        let bonsai = cs_id
            .load(self.ctx, self.repo.repo_blobstore())
            .await
            .with_context(|| format!("loading bonsai changeset {cs_id}"))?;

        if bonsai.is_snapshot() {
            bail!("cannot verify snapshot changeset {cs_id}");
        }

        let existing = self
            .fetch_required::<RootHgAugmentedManifestId>(cs_id)
            .await?;

        // Use stored parent roots so each changeset is compared independently
        // against persisted derived data.
        let mut aug_parents = Vec::new();
        for parent_cs_id in bonsai.parents() {
            let parent = self
                .fetch_required::<RootHgAugmentedManifestId>(parent_cs_id)
                .await
                .with_context(|| {
                    format!("fetching stored parent augmented root {parent_cs_id} for {cs_id}")
                })?;
            aug_parents.push(parent.hg_augmented_manifest_id());
        }

        let mapped_hg = self.fetch_required::<MappedHgChangesetId>(cs_id).await?;
        let expected_root = mapped_hg
            .hg_changeset_id()
            .load(self.ctx, self.repo.repo_blobstore())
            .await
            .with_context(|| format!("loading stored HgChangeset for {cs_id}"))?
            .manifestid()
            .into_nodehash();

        let acl_root = self.fetch_required::<RootAclManifestId>(cs_id).await?;
        let acl_overlay = normalize_acl_root(&acl_root)
            .with_context(|| format!("normalizing ACL root for {cs_id}"))?;

        let mem = MemWritesKeyedBlobstore::new(self.repo.repo_blobstore().clone());
        let acl_map = cached_acl_overlay_map(self.ctx, &mem, acl_overlay, &mut self.acl_cache)
            .await
            .with_context(|| format!("building ACL overlay map for {cs_id}"))?;

        let parent_acl_paths = self.parent_acl_paths(&mem, cs_id, &bonsai).await?;
        let source_aug_roots = self.subtree_source_aug_roots(cs_id, &bonsai).await?;

        let computed = derive_augmented_manifest_from_bonsai_changeset(
            self.ctx,
            &mem,
            &bonsai,
            aug_parents,
            &source_aug_roots,
            Some(expected_root),
            &self.restricted_paths,
            acl_overlay,
            &mut self.acl_cache,
        )
        .await
        .with_context(|| format!("directly deriving augmented manifest for {cs_id}"))?;

        let env_computed = computed
            .load(self.ctx, &mem)
            .await
            .with_context(|| format!("loading computed augmented manifest envelope for {cs_id}"))?;
        let env_stored = existing
            .hg_augmented_manifest_id()
            .load(self.ctx, self.repo.repo_blobstore())
            .await
            .with_context(|| format!("loading stored augmented manifest envelope for {cs_id}"))?;

        if let Some(mismatch) = compare_envelopes(cs_id, &env_computed, &env_stored) {
            return Ok(Some(mismatch));
        }

        // ACL pointer fields are not part of the augmented-manifest content
        // digest. Compare them over the sparse ACL overlay frontier for this
        // changeset and its parents, but intentionally avoid a full augmented
        // tree walk: this verifier is meant to run over large ranges.
        let all_acl_paths = acl_map
            .keys()
            .cloned()
            .chain(parent_acl_paths)
            .collect::<BTreeSet<_>>();
        if let Some(mismatch) = compare_acl_pointers(
            self.ctx,
            &mem,
            self.repo.repo_blobstore(),
            cs_id,
            &env_computed,
            &env_stored,
            all_acl_paths,
        )
        .await?
        {
            return Ok(Some(mismatch));
        }

        Ok(None)
    }

    async fn fetch_required<Derivable>(&self, cs_id: ChangesetId) -> Result<Derivable>
    where
        Derivable: derived_data_manager::BonsaiDerivable,
    {
        self.manager
            .fetch_derived::<Derivable>(self.ctx, cs_id, None)
            .await
            .with_context(|| format!("fetching stored {} for {cs_id}", Derivable::NAME))?
            .ok_or_else(|| anyhow!("{} is not derived for {cs_id}", Derivable::NAME))
    }

    async fn subtree_source_aug_roots(
        &self,
        cs_id: ChangesetId,
        bonsai: &mononoke_types::BonsaiChangeset,
    ) -> Result<HashMap<ChangesetId, HgAugmentedManifestId>> {
        let source_cs_ids = bonsai
            .subtree_changes()
            .into_iter()
            .filter_map(|(_dest_path, change)| {
                change
                    .copy_source()
                    .map(|(from_cs_id, _from_path)| from_cs_id)
            })
            .collect::<HashSet<_>>();

        if source_cs_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let source_cs_ids = source_cs_ids.into_iter().collect::<Vec<_>>();
        let fetched_sources = self
            .manager
            .fetch_derived_batch::<RootHgAugmentedManifestId>(self.ctx, source_cs_ids.clone(), None)
            .await
            .with_context(|| format!("fetching subtree source augmented roots for {cs_id}"))?;

        let mut sources = HashMap::new();
        for from_cs_id in source_cs_ids {
            let source = fetched_sources.get(&from_cs_id).ok_or_else(|| {
                anyhow!("subtree source augmented root {from_cs_id} is not derived for {cs_id}")
            })?;
            sources.insert(from_cs_id, source.hg_augmented_manifest_id());
        }
        Ok(sources)
    }

    async fn parent_acl_paths<Store>(
        &mut self,
        blobstore: &Store,
        cs_id: ChangesetId,
        bonsai: &mononoke_types::BonsaiChangeset,
    ) -> Result<BTreeSet<MPath>>
    where
        Store: KeyedBlobstore + 'static,
    {
        let mut paths = BTreeSet::new();
        for parent_cs_id in bonsai.parents() {
            let acl_root = self
                .fetch_required::<RootAclManifestId>(parent_cs_id)
                .await
                .with_context(|| format!("fetching parent ACL root {parent_cs_id} for {cs_id}"))?;
            let parent_overlay =
                normalize_acl_root(&acl_root).context("normalizing parent ACL root")?;
            let parent_map =
                cached_acl_overlay_map(self.ctx, blobstore, parent_overlay, &mut self.acl_cache)
                    .await
                    .context("building parent ACL overlay map")?;
            paths.extend(parent_map.keys().cloned());
        }
        Ok(paths)
    }
}

pub(crate) fn compare_envelopes(
    cs_id: ChangesetId,
    computed: &HgAugmentedManifestEnvelope,
    expected: &HgAugmentedManifestEnvelope,
) -> Option<VerifyMismatch> {
    if computed.augmented_manifest_id != expected.augmented_manifest_id {
        return Some(VerifyMismatch::new(
            cs_id,
            "augmented_manifest_id (Blake3)",
            format!("{:?}", computed.augmented_manifest_id),
            format!("{:?}", expected.augmented_manifest_id),
        ));
    }
    if computed.augmented_manifest_size != expected.augmented_manifest_size {
        return Some(VerifyMismatch::new(
            cs_id,
            "augmented_manifest_size",
            computed.augmented_manifest_size.to_string(),
            expected.augmented_manifest_size.to_string(),
        ));
    }
    if computed.augmented_manifest.hg_node_id != expected.augmented_manifest.hg_node_id {
        return Some(VerifyMismatch::new(
            cs_id,
            "hg_node_id",
            computed.augmented_manifest.hg_node_id.to_string(),
            expected.augmented_manifest.hg_node_id.to_string(),
        ));
    }
    if computed.augmented_manifest.computed_node_id != expected.augmented_manifest.computed_node_id
    {
        return Some(VerifyMismatch::new(
            cs_id,
            "computed_node_id",
            computed.augmented_manifest.computed_node_id.to_string(),
            expected.augmented_manifest.computed_node_id.to_string(),
        ));
    }
    if computed.augmented_manifest.p1 != expected.augmented_manifest.p1 {
        return Some(VerifyMismatch::new(
            cs_id,
            "p1",
            format!("{:?}", computed.augmented_manifest.p1),
            format!("{:?}", expected.augmented_manifest.p1),
        ));
    }
    if computed.augmented_manifest.p2 != expected.augmented_manifest.p2 {
        return Some(VerifyMismatch::new(
            cs_id,
            "p2",
            format!("{:?}", computed.augmented_manifest.p2),
            format!("{:?}", expected.augmented_manifest.p2),
        ));
    }
    if computed.augmented_manifest.acl_manifest_directory_id
        != expected.augmented_manifest.acl_manifest_directory_id
    {
        return Some(VerifyMismatch::new(
            cs_id,
            "acl_manifest_directory_id",
            format!(
                "{:?}",
                computed.augmented_manifest.acl_manifest_directory_id
            ),
            format!(
                "{:?}",
                expected.augmented_manifest.acl_manifest_directory_id
            ),
        ));
    }
    None
}

fn entry_kind(entry: &Option<HgAugmentedManifestEntry>) -> &'static str {
    match entry {
        None => "missing",
        Some(HgAugmentedManifestEntry::DirectoryNode(_)) => "directory",
        Some(HgAugmentedManifestEntry::FileNode(_)) => "file",
    }
}

/// Compare ACL pointers for the supplied sparse path set.
///
/// This is intentionally not a full augmented-manifest audit. Callers should
/// pass paths that can carry, or previously carried, ACL overlay pointers;
/// exhaustive recursive ACL-pointer parity is covered by derivation tests on
/// small fixtures, not by the large-range admin verifier.
pub(crate) async fn compare_acl_pointers(
    ctx: &CoreContext,
    computed_blobstore: &impl KeyedBlobstore,
    expected_blobstore: &impl KeyedBlobstore,
    cs_id: ChangesetId,
    computed: &HgAugmentedManifestEnvelope,
    expected: &HgAugmentedManifestEnvelope,
    acl_paths: BTreeSet<MPath>,
) -> Result<Option<VerifyMismatch>> {
    // Root acl_manifest_directory_id is already checked by compare_envelopes.
    // Walk non-root ACL overlay directories component-by-component using raw
    // lookup (ManifestOps conversion drops ACL pointers).
    for acl_path in acl_paths.into_iter().filter(|path| !path.is_root()) {
        if let Some(mismatch) = compare_acl_path_pointers(
            ctx,
            computed_blobstore,
            expected_blobstore,
            cs_id,
            computed,
            expected,
            &acl_path,
        )
        .await?
        {
            return Ok(Some(mismatch));
        }
    }

    Ok(None)
}

async fn compare_acl_path_pointers(
    ctx: &CoreContext,
    computed_blobstore: &impl KeyedBlobstore,
    expected_blobstore: &impl KeyedBlobstore,
    cs_id: ChangesetId,
    computed: &HgAugmentedManifestEnvelope,
    expected: &HgAugmentedManifestEnvelope,
    acl_path: &MPath,
) -> Result<Option<VerifyMismatch>> {
    let components = acl_path.iter().collect::<Vec<_>>();

    let mut cur_computed = computed.clone();
    let mut cur_expected = expected.clone();
    for (depth, elem) in components.iter().enumerate() {
        let entry_computed = cur_computed
            .augmented_manifest
            .lookup(ctx, computed_blobstore, elem)
            .await
            .with_context(|| format!("looking up computed ACL path {acl_path}"))?;
        let entry_expected = cur_expected
            .augmented_manifest
            .lookup(ctx, expected_blobstore, elem)
            .await
            .with_context(|| format!("looking up expected ACL path {acl_path}"))?;

        match (entry_computed, entry_expected) {
            (
                Some(HgAugmentedManifestEntry::DirectoryNode(dir_computed)),
                Some(HgAugmentedManifestEntry::DirectoryNode(dir_expected)),
            ) => {
                let partial_path =
                    MPath::from_elements(components[..=depth].iter().copied()).to_string();

                if dir_computed.acl_manifest_directory_id != dir_expected.acl_manifest_directory_id
                {
                    return Ok(Some(VerifyMismatch::new(
                        cs_id,
                        format!("entry acl_manifest_directory_id at {partial_path}"),
                        format!("{:?}", dir_computed.acl_manifest_directory_id),
                        format!("{:?}", dir_expected.acl_manifest_directory_id),
                    )));
                }

                let child_computed = HgAugmentedManifestId::new(dir_computed.treenode)
                    .load(ctx, computed_blobstore)
                    .await
                    .with_context(|| {
                        format!("loading computed child envelope at {partial_path}")
                    })?;
                let child_expected = HgAugmentedManifestId::new(dir_expected.treenode)
                    .load(ctx, expected_blobstore)
                    .await
                    .with_context(|| {
                        format!("loading expected child envelope at {partial_path}")
                    })?;

                if child_computed.augmented_manifest.acl_manifest_directory_id
                    != child_expected.augmented_manifest.acl_manifest_directory_id
                {
                    return Ok(Some(VerifyMismatch::new(
                        cs_id,
                        format!("envelope acl_manifest_directory_id at {partial_path}"),
                        format!(
                            "{:?}",
                            child_computed.augmented_manifest.acl_manifest_directory_id
                        ),
                        format!(
                            "{:?}",
                            child_expected.augmented_manifest.acl_manifest_directory_id
                        ),
                    )));
                }

                if depth < components.len() - 1 {
                    cur_computed = child_computed;
                    cur_expected = child_expected;
                }
            }
            (None, None)
            | (
                Some(HgAugmentedManifestEntry::FileNode(_)),
                Some(HgAugmentedManifestEntry::FileNode(_)),
            ) => break,
            (entry_c, entry_s) => {
                let partial_path =
                    MPath::from_elements(components[..=depth].iter().copied()).to_string();
                return Ok(Some(VerifyMismatch::new(
                    cs_id,
                    format!("acl entry type at {partial_path}"),
                    entry_kind(&entry_c).to_string(),
                    entry_kind(&entry_s).to_string(),
                )));
            }
        }
    }

    Ok(None)
}

const DEFAULT_VERIFY_AUG_DIRECT_BATCH_SIZE: u64 = 100;
const MAX_VERIFY_AUG_DIRECT_BATCH_SIZE: u64 = 10_000;
const MAX_VERIFY_AUG_DIRECT_CONCURRENCY: u64 = 100;

#[derive(Args)]
pub(super) struct VerifyAugDirectArgs {
    /// Inclusive upper bound changeset (hex bonsai changeset ID).
    /// Mutually exclusive with --bookmark.
    #[clap(long, conflicts_with = "bookmark")]
    end_id: Option<ChangesetId>,

    /// Resolve upper bound from a bookmark (e.g. master).
    /// Mutually exclusive with --end-id. Defaults to 'master' when neither is set.
    #[clap(long, short = 'B', conflicts_with = "end_id")]
    bookmark: Option<BookmarkKey>,

    /// Number of most recent changesets to validate, ending at --end-id/--bookmark.
    #[clap(long, value_parser = clap::value_parser!(u64).range(1..))]
    last: u64,

    /// Number of changesets to select per verifier batch. Default: 100. Maximum: 10000.
    #[clap(
        long,
        default_value_t = DEFAULT_VERIFY_AUG_DIRECT_BATCH_SIZE,
        value_parser = clap::value_parser!(u64).range(1..=MAX_VERIFY_AUG_DIRECT_BATCH_SIZE),
    )]
    batch_size: u64,

    /// Number of verifier batches to process concurrently. Maximum: 100.
    #[clap(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=MAX_VERIFY_AUG_DIRECT_CONCURRENCY))]
    concurrency: u64,
}

async fn resolve_end_id(
    ctx: &CoreContext,
    repo: &Repo,
    end_id: Option<ChangesetId>,
    bookmark: Option<BookmarkKey>,
) -> Result<ChangesetId> {
    match end_id {
        Some(end_id) => Ok(end_id),
        None => {
            let bookmark = match bookmark {
                Some(bookmark) => bookmark,
                None => BookmarkKey::new("master")?,
            };
            repo.bookmarks()
                .get(ctx.clone(), &bookmark, Freshness::MostRecent)
                .await?
                .ok_or_else(|| anyhow!("bookmark '{bookmark}' not found"))
        }
    }
}

struct VerifyBatch {
    index: u64,
    cs_ids: Vec<ChangesetId>,
}

fn batch_changeset_stream(
    cs_ids: BoxStream<'static, Result<ChangesetId>>,
    batch_size: u64,
) -> Result<BoxStream<'static, Result<VerifyBatch>>> {
    let batch_size =
        usize::try_from(batch_size).context("--batch-size value does not fit in usize")?;
    Ok(cs_ids
        .try_chunks(batch_size)
        .enumerate()
        .map(|(index, batch)| match batch {
            Ok(cs_ids) => Ok(VerifyBatch {
                index: index as u64 + 1,
                cs_ids,
            }),
            Err(err) => Err(err.1),
        })
        .boxed())
}

async fn select_last_changesets(
    ctx: &CoreContext,
    repo: &Repo,
    end_id: ChangesetId,
    count: u64,
) -> Result<BoxStream<'static, Result<ChangesetId>>> {
    let count = usize::try_from(count).context("--last value does not fit in usize")?;
    Ok(repo
        .commit_graph()
        .ancestors_difference_stream(ctx, vec![end_id], vec![])
        .await?
        .take(count)
        .boxed())
}

async fn select_last_changeset_batches(
    ctx: &CoreContext,
    repo: &Repo,
    end_id: ChangesetId,
    count: u64,
    batch_size: u64,
) -> Result<BoxStream<'static, Result<VerifyBatch>>> {
    let cs_ids = select_last_changesets(ctx, repo, end_id, count).await?;
    batch_changeset_stream(cs_ids, batch_size)
}

async fn verify_aug_direct_batch(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    batch: VerifyBatch,
) -> Result<(u64, u64)> {
    let index = batch.index;
    let size = u64::try_from(batch.cs_ids.len()).context("batch size does not fit in u64")?;
    let mut verifier = Verifier::new(ctx, repo, manager);

    for cs_id in batch.cs_ids {
        if let Some(m) = verifier.verify_one(cs_id).await? {
            return Err(anyhow!(
                "MISMATCH at {}: {} diverges: computed={} expected={}",
                m.cs_id,
                m.field,
                m.computed,
                m.expected,
            ));
        }
    }

    Ok((index, size))
}

pub(super) async fn verify_aug_direct(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: VerifyAugDirectArgs,
) -> Result<()> {
    let end_id = resolve_end_id(ctx, repo, args.end_id, args.bookmark).await?;
    let batches =
        select_last_changeset_batches(ctx, repo, end_id, args.last, args.batch_size).await?;

    info!("verifying up to {} changesets", args.last);

    let concurrency =
        usize::try_from(args.concurrency).context("--concurrency value does not fit in usize")?;
    let mut processed = 0;

    batches
        .map_ok(|batch| verify_aug_direct_batch(ctx, repo, manager, batch))
        .try_buffer_unordered(concurrency)
        .try_for_each(|(batch_index, batch_size)| {
            processed += batch_size;
            info!("progress: batch={batch_index} size={batch_size} processed={processed}");
            future::ready(Ok(()))
        })
        .await?;

    println!("done: processed={processed}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bonsai_git_mapping::BonsaiGitMapping;
    use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
    use bookmarks::Bookmarks;
    use changesets_creation::save_changesets;
    use clap::Parser;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filenodes::Filenodes;
    use filestore::FilestoreConfig;
    use futures::TryStreamExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::override_just_knobs;
    use mercurial_types::HgNodeHash;
    use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
    use mercurial_types::sharded_augmented_manifest::ShardedHgAugmentedManifest;
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::hash::Blake2;
    use mononoke_types::hash::Blake3;
    use mononoke_types::hash::Sha1 as ContentSha1;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;
    use mononoke_types::sharded_map_v2::ShardedMapV2Node;
    use mononoke_types::subtree_change::SubtreeChange;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tests_utils::CreateCommitContext;

    use super::*;

    const SLACL_PROJECT1: &[u8] =
        b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n";

    #[facet::container]
    struct TestRepo {
        #[delegate(
            dyn BonsaiHgMapping,
            dyn BonsaiGitMapping,
            dyn BonsaiGlobalrevMapping,
            dyn BonsaiSvnrevMapping,
            RepoBlobstore,
            dyn Bookmarks,
            CommitGraph,
            dyn Filenodes,
            FilestoreConfig,
            RepoDerivedData,
            RepoIdentity,
        )]
        repo: Repo,

        #[facet]
        commit_graph_writer: dyn CommitGraphWriter,
    }

    fn knob_overrides(extra: impl IntoIterator<Item = (&'static str, bool)>) -> JustKnobsInMemory {
        let mut knobs = HashMap::from([(
            "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
            KnobVal::Bool(false),
        )]);
        knobs.extend(
            extra
                .into_iter()
                .map(|(name, value)| (name.to_string(), KnobVal::Bool(value))),
        );
        JustKnobsInMemory::new(knobs)
    }

    async fn derive_stored_aug(
        ctx: &CoreContext,
        repo: &Repo,
        cs_ids: &[ChangesetId],
    ) -> Result<()> {
        let manager = repo.repo_derived_data().manager();
        manager
            .derive_exactly_batch::<MappedHgChangesetId>(ctx, cs_ids.to_vec(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootAclManifestId>(ctx, cs_ids.to_vec(), None)
            .await?;
        manager
            .derive_exactly_batch::<RootHgAugmentedManifestId>(ctx, cs_ids.to_vec(), None)
            .await?;
        Ok(())
    }

    fn test_changeset_id() -> ChangesetId {
        ChangesetId::new(Blake2::from_byte_array([0x99; 32]))
    }

    fn test_acl_id(byte: u8) -> AclManifestId {
        AclManifestId::new(Blake2::from_byte_array([byte; 32]))
    }

    fn test_envelope(byte: u8, acl_id: Option<AclManifestId>) -> HgAugmentedManifestEnvelope {
        let node_hash = HgNodeHash::new(NodeSha1::from_byte_array([byte; 20]));
        HgAugmentedManifestEnvelope {
            augmented_manifest_id: Blake3::from_byte_array([byte; 32]),
            augmented_manifest_size: byte as u64,
            augmented_manifest: ShardedHgAugmentedManifest {
                hg_node_id: node_hash,
                p1: None,
                p2: None,
                computed_node_id: node_hash,
                subentries: ShardedMapV2Node::default(),
                acl_manifest_directory_id: acl_id,
            },
        }
    }

    fn test_file_entry(byte: u8) -> HgAugmentedManifestEntry {
        HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode {
            file_type: FileType::Regular,
            filenode: HgNodeHash::new(NodeSha1::from_byte_array([byte; 20])),
            content_blake3: Blake3::from_byte_array([byte; 32]),
            content_sha1: ContentSha1::from_byte_array([byte; 20]),
            total_size: byte as u64,
            file_header_metadata: None,
        })
    }

    #[derive(Parser)]
    struct VerifyAugDirectArgsParser {
        #[clap(flatten)]
        _args: VerifyAugDirectArgs,
    }

    #[mononoke::test]
    fn verify_aug_direct_args_rejects_excessive_concurrency() {
        // Given a concurrency value above the operational safety cap.
        let excessive_concurrency = (MAX_VERIFY_AUG_DIRECT_CONCURRENCY + 1).to_string();

        // When parsing the verifier arguments.
        let error = match VerifyAugDirectArgsParser::try_parse_from([
            "verify-aug-direct",
            "--last",
            "1",
            "--concurrency",
            excessive_concurrency.as_str(),
        ]) {
            Ok(_) => panic!("concurrency above the cap should be rejected"),
            Err(error) => error,
        };

        // Then clap rejects the value before any verification work can fan out.
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(error.to_string().contains(&format!(
            "is not in 1..={MAX_VERIFY_AUG_DIRECT_CONCURRENCY}"
        )));
    }

    #[mononoke::fbinit_test]
    async fn select_last_changeset_batches_groups_selected_changesets_into_bounded_batches(
        fb: FacebookInit,
    ) -> Result<()> {
        // Given a linear repository history with five changesets.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", b"one")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("file.txt", b"two")
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .add_file("file.txt", b"three")
            .commit()
            .await?;
        let fourth = CreateCommitContext::new(&ctx, &repo, vec![third])
            .add_file("file.txt", b"four")
            .commit()
            .await?;
        let fifth = CreateCommitContext::new(&ctx, &repo, vec![fourth])
            .add_file("file.txt", b"five")
            .commit()
            .await?;

        // When selecting the last five changesets in batches of two.
        let batches = select_last_changeset_batches(&ctx, &repo.repo, fifth, 5, 2)
            .await?
            .map_ok(|batch| batch.cs_ids)
            .try_collect::<Vec<_>>()
            .await?;

        // Then the selected changesets are emitted in bounded batches.
        assert_eq!(
            batches,
            vec![vec![fifth, fourth], vec![third, second], vec![root]]
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn compare_envelopes_detects_mismatch(_fb: FacebookInit) -> Result<()> {
        let cs_id = test_changeset_id();
        let expected = test_envelope(1, None);
        let matching = expected.clone();
        let different = test_envelope(2, None);

        assert_eq!(compare_envelopes(cs_id, &matching, &expected), None);
        let mismatch = compare_envelopes(cs_id, &different, &expected)
            .ok_or_else(|| anyhow!("expected envelope mismatch"))?;
        assert_eq!(mismatch.field, "augmented_manifest_id (Blake3)");

        let acl_a = test_envelope(1, Some(test_acl_id(1)));
        let acl_b = test_envelope(1, Some(test_acl_id(2)));
        let mismatch = compare_envelopes(cs_id, &acl_a, &acl_b)
            .ok_or_else(|| anyhow!("expected ACL mismatch"))?;
        assert_eq!(mismatch.field, "acl_manifest_directory_id");
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn verify_one_validates_acl_pointer_changeset(fb: FacebookInit) -> Result<()> {
        // Given a changeset with an ACL-bearing directory and stored old-path derived data.
        override_just_knobs(knob_overrides([(
            "scm/mononoke:add_acl_manifest_pointer",
            true,
        )]));
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir/file.txt", b"content")
            .add_file("dir/.slacl", SLACL_PROJECT1)
            .commit()
            .await?;
        derive_stored_aug(&ctx, &repo.repo, &[root]).await?;
        let manager = repo.repo.repo_derived_data().manager();
        let acl_root = manager
            .fetch_derived::<RootAclManifestId>(&ctx, root, None)
            .await?
            .ok_or_else(|| anyhow!("missing RootAclManifestId for {root}"))?;
        let acl_overlay = normalize_acl_root(&acl_root)?;
        let mem = MemWritesKeyedBlobstore::new(repo.repo.repo_blobstore().clone());
        let acl_map = cached_acl_overlay_map(&ctx, &mem, acl_overlay, &mut HashMap::new()).await?;
        assert!(!acl_map.is_empty());

        // When verifying the changeset.
        let mut verifier = Verifier::new(&ctx, &repo.repo, manager);
        let outcome = verifier.verify_one(root).await?;

        // Then direct derivation matches the stored data, including ACL pointers.
        assert_eq!(outcome, None);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn compare_acl_pointers_accepts_matching_files(fb: FacebookInit) -> Result<()> {
        // Given computed and stored manifests that both resolve a selected ACL path to a file.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let subentries = ShardedMapV2Node::from_entries(
            &ctx,
            repo.repo.repo_blobstore(),
            [(b"dir".as_slice(), test_file_entry(1))],
        )
        .await?;
        let mut computed = test_envelope(1, None);
        computed.augmented_manifest.subentries = subentries.clone();
        let mut expected = test_envelope(1, None);
        expected.augmented_manifest.subentries = subentries;

        // When comparing ACL pointers for that path.
        let result = compare_acl_pointers(
            &ctx,
            repo.repo.repo_blobstore(),
            repo.repo.repo_blobstore(),
            test_changeset_id(),
            &computed,
            &expected,
            BTreeSet::from([MPath::new("dir")?]),
        )
        .await?;

        // Then matching file/file entries do not cause an ACL mismatch.
        assert_eq!(result, None);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn verify_one_rejects_snapshot_changesets(fb: FacebookInit) -> Result<()> {
        // Given a snapshot changeset selected for augmented-manifest validation.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let mut bonsai = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", b"content")
            .create_commit_object()
            .await?;
        bonsai.is_snapshot = true;
        let bonsai = bonsai.freeze()?;
        let cs_id = bonsai.get_changeset_id();
        save_changesets(&ctx, &repo, vec![bonsai]).await?;
        let manager = repo.repo.repo_derived_data().manager();
        let mut verifier = Verifier::new(&ctx, &repo.repo, manager);

        // When validating the snapshot changeset.
        let error = verifier
            .verify_one(cs_id)
            .await
            .expect_err("snapshot changesets should abort validation");

        // Then validation aborts with the selected changeset id instead of skipping it.
        assert!(
            format!("{error:#}").contains(&format!("cannot verify snapshot changeset {cs_id}"))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn verify_one_end_to_end(fb: FacebookInit) -> Result<()> {
        override_just_knobs(knob_overrides([]));
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", b"one")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("file.txt", b"two")
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .add_file("other.txt", b"three")
            .commit()
            .await?;
        let cs_ids = vec![root, second, third];
        derive_stored_aug(&ctx, &repo.repo, &cs_ids).await?;

        let manager = repo.repo.repo_derived_data().manager();
        let mut verifier = Verifier::new(&ctx, &repo.repo, manager);
        for cs_id in cs_ids {
            assert_eq!(verifier.verify_one(cs_id).await?, None);
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn verify_one_validates_subtree_copy_only_changeset(fb: FacebookInit) -> Result<()> {
        override_just_knobs(knob_overrides([
            ("scm/mononoke:enable_subtree_changes", true),
            (
                "scm/mononoke:enable_manifest_altering_subtree_changes",
                true,
            ),
        ]));
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("src/file.txt", b"one")
            .commit()
            .await?;

        let mut bonsai = CreateCommitContext::new(&ctx, &repo, vec![root])
            .set_message("subtree copy only")
            .create_commit_object()
            .await?;
        bonsai.subtree_changes = vec![(
            MPath::new("dst")?,
            SubtreeChange::copy(MPath::new("src")?, root),
        )]
        .into_iter()
        .collect();
        let bonsai = bonsai.freeze()?;
        let child = bonsai.get_changeset_id();
        save_changesets(&ctx, &repo, vec![bonsai]).await?;

        derive_stored_aug(&ctx, &repo.repo, &[root, child]).await?;

        let manager = repo.repo.repo_derived_data().manager();
        let mut verifier = Verifier::new(&ctx, &repo.repo, manager);
        assert_eq!(verifier.verify_one(child).await?, None);
        Ok(())
    }
}
