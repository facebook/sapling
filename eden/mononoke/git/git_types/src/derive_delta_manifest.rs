/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Ok;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::Loadable;
use blobstore::Storable;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use filestore::fetch_with_size;
use futures_util::future::try_join_all;
use futures_util::stream;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use gix_diff::blob::Algorithm;
use gix_hash::ObjectId;
use manifest::ManifestOps;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use multimap::MultiMap;
use unodes::RootUnodeManifestId;

use crate::delta::DeltaInstructionChunkIdPrefix;
use crate::delta::DeltaInstructions;
use crate::delta_manifest::GitDeltaManifest;
use crate::delta_manifest::GitDeltaManifestEntry;
use crate::delta_manifest::GitDeltaManifestId;
use crate::delta_manifest::ObjectDelta;
use crate::delta_manifest::ObjectEntry;
use crate::store::fetch_git_object_bytes;
use crate::store::store_delta_instructions;
use crate::DeltaObjectKind;
use crate::GitError;
use crate::MappedGitCommitId;
use crate::TreeHandle;
use crate::TreeMember;

// TODO(rajsha): Move these to DerivedDataConfig for GitDeltaManifest
/// Chunk size used for chunking whole DeltaInstructions object, i.e. if size of DeltaInstructions is 64MB
/// then it will be stored as ~16 chunks of ~4MB (64MB / ~4MB).
const CHUNK_SIZE: u64 = 4_193_280; // little less than 4MB
/// The object size threshold beyond which we do not consider the object for deltafication
const DELTA_THRESHOLD: u64 = 262_144_000; // 250 MB

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct RootGitDeltaManifestId(GitDeltaManifestId);

impl RootGitDeltaManifestId {
    pub fn new(id: GitDeltaManifestId) -> Self {
        Self(id)
    }
}

impl TryFrom<BlobstoreBytes> for RootGitDeltaManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        GitDeltaManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootGitDeltaManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootGitDeltaManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootGitDeltaManifestId> for BlobstoreBytes {
    fn from(root_gdm_id: RootGitDeltaManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_gdm_id.0.blake2().as_ref()))
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_git_delta_manifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootGitDeltaManifestId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

fn tree_member_to_object_entry(member: &TreeMember, path: MPath) -> Result<ObjectEntry> {
    let rich_git_sha1 = member.oid();
    let oid = ObjectId::from_hex(rich_git_sha1.to_hex().as_bytes()).with_context(|| {
        format!(
            "Error while converting hash {:?} to ObjectId",
            rich_git_sha1.to_hex()
        )
    })?;
    let size = rich_git_sha1.size();
    let kind = match member.kind() {
        crate::ObjectKind::Blob => DeltaObjectKind::Blob,
        crate::ObjectKind::Tree => DeltaObjectKind::Tree,
        kind => anyhow::bail!("Unexpected object kind {:?} for DeltaObjectEntry", kind),
    };
    Ok(ObjectEntry {
        oid,
        size,
        kind,
        path,
    })
}

async fn get_object_bytes(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    entry: &ObjectEntry,
    sha: &RichGitSha1,
) -> Result<Bytes> {
    match entry.kind {
        // Blobs are stored as regular content in Mononoke and can be accessed via GitSha1 alias
        DeltaObjectKind::Blob => {
            let fetch_key = sha.clone().into();
            let (bytes_stream, num_bytes) = fetch_with_size(blobstore, ctx, &fetch_key)
                .await
                .map_err(|e| GitError::StorageFailure(sha.to_hex().to_string(), e.into()))?
                .ok_or_else(|| GitError::NonExistentObject(sha.to_hex().to_string()))?;
            // The blob object stored in the blobstore exists without the git header. Prepend the git blob header before retuning the bytes
            let mut header_bytes = sha.prefix();
            // We know the number of bytes we are going to write so reserve the buffer to avoid resizing
            header_bytes.reserve(num_bytes as usize);
            bytes_stream
                .try_fold(header_bytes, |mut acc, bytes| async move {
                    acc.append(&mut bytes.to_vec());
                    anyhow::Ok(acc)
                })
                .await
                .map(Bytes::from)
        }
        // Trees are stored directly as raw Git trees in Mononoke
        DeltaObjectKind::Tree => {
            let object_bytes = fetch_git_object_bytes(ctx, &blobstore, entry.oid.as_ref()).await?;
            Ok(object_bytes)
        }
    }
}

async fn metadata_to_manifest_entry(
    commit: &ChangesetId,
    path: MPath,
    metadata: DeltaEntryMetadata,
    blobstore: Arc<dyn Blobstore>,
    ctx: &CoreContext,
) -> Result<GitDeltaManifestEntry> {
    let full_object_entry = tree_member_to_object_entry(&metadata.actual, path.clone())
        .with_context(|| {
            format!(
                "Error while converting TreeMember {:?} to ObjectEntry",
                metadata.actual
            )
        })?;
    let deltas = stream::iter(metadata.deltas.into_iter())
        .map(|delta_metadata| {
            cloned!(path, blobstore, commit, ctx, full_object_entry);
            // These are deep nested futures with fair bit of CPU bound work. Using tokio::spawn ensures they get polled. This won't
            // result in too many spawned futures since there will be less than 5 deltas for 99% cases
            tokio::spawn(async move {
                let base = tree_member_to_object_entry(&delta_metadata.object, path.clone())
                    .with_context(|| {
                        format!(
                            "Error while converting TreeMember {:?} to ObjectEntry",
                            delta_metadata.object
                        )
                    })?;
                let origin = delta_metadata.origin;
                let actual_object = get_object_bytes(&ctx, blobstore.clone(), &full_object_entry, metadata.actual.oid()).await?;
                let base_object = get_object_bytes(&ctx, blobstore.clone(), &base, delta_metadata.object.oid()).await?;
                let instructions = DeltaInstructions::generate(
                    base_object,
        actual_object,
    Algorithm::Myers,
                )
                .with_context(|| {
                    format!(
                        "Error while computing delta between base object {:?} and actual object {:?}",
                        base.oid, full_object_entry.oid
                    )
                })?;
                // The base path and actual path are the same for now but can vary in the future when we support
                // files copied from one location to the other
                let chunk_prefix =
                    DeltaInstructionChunkIdPrefix::new(commit, path.clone(), origin, path.clone());
                let chunk_size = Some(CHUNK_SIZE);
                let instructions_chunk_count = store_delta_instructions(&ctx, &blobstore, instructions, chunk_prefix, chunk_size)
                    .await
                    .with_context(|| {
                        format!(
                            "Error while storing delta instructions for path {} in commit {}",
                            path, commit
                        )
                    })?;
                anyhow::Ok(ObjectDelta::new(origin, base, instructions_chunk_count))
            })
        })
        .buffer_unordered(20) // There will mostly be 1-2 deltas per path per object so concurrency of 20 is more than enough
        .try_collect::<Vec<_>>()
        .await?.into_iter().collect::<Result<Vec<_>>>()?;
    Ok(GitDeltaManifestEntry::new(full_object_entry, deltas))
}

/// Struct representing the metadata of a Git tree manifest entry
#[derive(Debug, Clone, Eq, PartialEq)]
struct DeltaEntryMetadata {
    actual: TreeMember,
    deltas: HashSet<DeltaMetadata>,
}

impl DeltaEntryMetadata {
    /// Create a new non-delta `DeltaEntryMetadata` from a given `TreeMember`
    fn new(actual: TreeMember) -> Self {
        Self {
            actual,
            deltas: HashSet::new(),
        }
    }

    /// Create a new `DeltaEntryMetadata` with both base and delta object information
    fn with_delta(
        actual: TreeMember,
        base_obj_for_delta: TreeMember,
        base_commit_for_delta: ChangesetId,
    ) -> Self {
        Self {
            actual,
            deltas: HashSet::from([DeltaMetadata::new(
                base_obj_for_delta,
                base_commit_for_delta,
            )]),
        }
    }

    /// Is the current entry a delta entry? (i.e. can it be expressed as a delta of a base object?)
    fn is_delta_entry(&self) -> bool {
        !self.deltas.is_empty()
    }

    /// Merge the given list of DeltaEntryMetadata into a single DeltaEntryMetadata
    /// Requires all entries to be deltas and have the same actual object
    fn merged_entry(entries: Vec<DeltaEntryMetadata>) -> Result<Self> {
        entries
            .into_iter()
            .try_reduce(|mut acc, entry| {
                if acc.actual != entry.actual {
                    return Err(anyhow!(
                        "All entries must have the same actual object for merging"
                    ));
                } else if !entry.is_delta_entry() {
                    return Err(anyhow!(
                        "Only delta entries can be merged into other delta entries"
                    ));
                } else {
                    acc.deltas.extend(entry.deltas.into_iter());
                }
                Ok(acc)
            })?
            .ok_or_else(|| anyhow::anyhow!("No DeltaEntryMetadata entries found to merge"))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct DeltaMetadata {
    origin: ChangesetId,
    object: TreeMember,
}

impl DeltaMetadata {
    fn new(object: TreeMember, origin: ChangesetId) -> Self {
        Self { object, origin }
    }
}

#[async_trait]
impl BonsaiDerivable for RootGitDeltaManifestId {
    const VARIANT: DerivableType = DerivableType::GitDeltaManifest;

    type Dependencies = dependencies![TreeHandle, MappedGitCommitId, RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        if bonsai.is_snapshot() {
            anyhow::bail!("Can't derive GitDeltaManifest for snapshot")
        }
        // Derive the Git tree manifest for the current commit
        let tree_handle = derivation_ctx
            .derive_dependency::<TreeHandle>(ctx, bonsai.get_changeset_id())
            .await
            .context("Error while deriving current commit's Git tree")?;
        // For each parent of the bonsai changeset, derive the Git tree manifest
        // Ok to try join since there will only be a handful of parents for Git repos
        let parent_trees_with_commit = try_join_all(bonsai.parents().map(|parent| async move {
            let parent_tree_handle = derivation_ctx
                .derive_dependency::<TreeHandle>(ctx, parent.clone())
                .await
                .with_context(|| {
                    format!("Error while deriving Git tree for parent commit {}", parent)
                })?;
            anyhow::Ok((parent, parent_tree_handle))
        }))
        .await?;

        // Perform a manifest diff between the parent and the current changeset to identify the paths (could be file or directory)
        // that have been modified in the current commit as compared to the parent commit. Collect the result in a Multimap that
        // maps from MPath (added or modified) to Vec<DeltaEntryMetadata>. In case of added MPath, there would be only one DeltaEntryMetadata
        // value but for modified paths (by different parents), there will be multiple DeltaEntryMetadata values.
        let diff_items = stream::iter(parent_trees_with_commit)
            .flat_map(|(parent_changeset_id, parent_tree_handle)| {
                // Diff the Git tree of the parent with the current commits Git tree. This will give information about
                // what paths were added, modified or deleted
                parent_tree_handle.filtered_diff(
                    ctx.clone(),
                    derivation_ctx.blobstore().clone(),
                    tree_handle.clone(),
                    derivation_ctx.blobstore().clone(),
                    move |diff_entry| {
                        match diff_entry {
                            // We only care about files/directories that were added or modified since removed entries won't be
                            // included in GitDeltaManifest
                            manifest::Diff::Added(path, entry) => Some((
                                path,
                                // Transform to TreeMember so we can easily access type, size and oid information
                                DeltaEntryMetadata::new(TreeMember::from(entry)),
                            )),
                            manifest::Diff::Changed(path, old_entry, new_entry) => {
                                let actual = TreeMember::from(new_entry);
                                let base = TreeMember::from(old_entry);
                                if actual.oid().size() > DELTA_THRESHOLD
                                    || base.oid().size() > DELTA_THRESHOLD
                                {
                                    // If either the base object or the actual object is too large, then we don't want to delta them
                                    // and instead use them directly
                                    Some((
                                        path,
                                        DeltaEntryMetadata::new(TreeMember::from(new_entry)),
                                    ))
                                } else {
                                    Some((
                                        path,
                                        // The parent changeset id is _not really_ the changeset that introduced the base object (i.e. old entry)
                                        // but we still use it here since it tells us which commit's unode we need to look at, to find the actual
                                        // base commit that introduced this object (tree or file)
                                        DeltaEntryMetadata::with_delta(
                                            actual,
                                            base,
                                            parent_changeset_id,
                                        ),
                                    ))
                                }
                            }
                            // Ignore entries with no path or deleted entries
                            _ => None,
                        }
                    },
                    |_| true, // recurse_pruner is a function that allows us to skip iterating over some subtrees
                )
            })
            // Collect as a MultiMap since the same modification can potentially be made as part of different parents
            .try_collect::<MultiMap<_, _>>()
            .await?;

        // The MultiMap contains a map of MPath -> Vec<DeltaEntryMetadata> since a modified path can have potentially multiple bases to
        // create delta and each such base object will have its own associated commit (e.g. merge-commit containing file/directory modified in multiple parents).
        // Since DeltaEntryMetadata can represent multiple base objects with their commits, lets merge Vec<DeltaEntryMetadata> into a single DeltaEntryMetadata
        // that represents the entire delta
        let diff_items = diff_items
            .into_iter()
            .map(|(path, entries)| Ok((path, DeltaEntryMetadata::merged_entry(entries)?)))
            .collect::<Result<HashMap<_, _>>>()?;

        // To determine the actual source commit of the delta base object, we need to look into the parent commit unodes for the files/directories
        // that were modified in the current commit
        let parent_unodes_with_commit = try_join_all(bonsai.parents().map(|parent| async move {
            let parent_root_unode = derivation_ctx
                .derive_dependency::<RootUnodeManifestId>(ctx, parent.clone())
                .await
                .with_context(|| {
                    format!(
                        "Error while deriving root unode for parent commit {}",
                        parent
                    )
                })?;
            anyhow::Ok((parent, parent_root_unode))
        }))
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

        // For each modified path, find the correct origin commit that introduced the previous modification to the path and generate the delta entries
        let manifest_entries = stream::iter(diff_items.into_iter()).map(|(path, mut entry)| {
            let parent_unodes_with_commit = &parent_unodes_with_commit;
            let commit = bonsai.get_changeset_id();
            let blobstore = derivation_ctx.blobstore().clone();
            async move {
                // Need to look at unodes only if this is a delta entry (i.e. entry for a modified file or directory)
                if entry.is_delta_entry() {
                    // HashSet for storing the DeltaEntries with correct origin commit
                    let mut deltas_with_correct_origin = HashSet::new();
                    for delta_entry in entry.deltas.into_iter() {
                        // Currently the origin for the DeltaMetadata is the parent commit that was diffed against the current
                        // commit to get this modified path. This parent might or might not be the right commit when the file was last
                        // modified before the current commit. The parent commit's unode would give the right origin commit for this path
                        let root_unode = parent_unodes_with_commit
                            .get(&delta_entry.origin)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Root unode not found for origin commit {:?}",
                                    delta_entry.origin
                                )
                            })?;
                        let mut unodes = root_unode
                            .manifest_unode_id()
                            .find_entries(
                                ctx.clone(),
                                derivation_ctx.blobstore().clone(),
                                vec![path.clone()],
                            )
                            .try_collect::<Vec<_>>()
                            .await.with_context(|| format!("Error in finding entries for path {:?} in root unode for origin commit {:?}", path, delta_entry.origin))?;
                        let (returned_path, unode_entry) = unodes.pop().ok_or_else(|| {
                            anyhow::anyhow!(
                                "No unode found for path {:?} in origin commit {:?}",
                                path,
                                delta_entry.origin
                            )
                        })?;
                        if returned_path != path {
                            anyhow::bail!("Unexpected path {:?} found in unode for path {:?} in origin commit {:?}", returned_path, path, delta_entry.origin);
                        }
                        let updated_entry = match unode_entry {
                            manifest::Entry::Tree(tree_id) => {
                                let manifest_unode = tree_id.load(ctx, derivation_ctx.blobstore())
                                        .await.with_context(|| format!("Error in loading manifest unode {:?} for path {:?} and origin commit {:?}", tree_id, path, delta_entry.origin))?;
                                // Set the correct origin commit returned by the manifest unode
                                DeltaMetadata::new(delta_entry.object, manifest_unode.linknode().clone())
                            },
                            manifest::Entry::Leaf(file_id) => {
                                let file_unode = file_id.load(ctx, derivation_ctx.blobstore())
                                        .await.with_context(|| format!("Error in loading file unode {:?} for path {:?} and origin commit {:?}", file_id, path, delta_entry.origin))?;
                                // Set the correct origin commit returned by the file unode
                                DeltaMetadata::new(delta_entry.object, file_unode.linknode().clone())
                            },
                        };
                        deltas_with_correct_origin.insert(updated_entry);
                    }
                    entry.deltas = deltas_with_correct_origin;
                }
                // Use the metadata of the delta entry to construct GitDeltaManifestEntry
                let manifest_entry = metadata_to_manifest_entry(&commit, path.clone(), entry, blobstore, ctx)
                        .await.with_context(|| format!("Error in generating git delta manifest entry for path {}", path))?;
                anyhow::Ok((path, manifest_entry))
            }
        })
        .buffered(100)
        .try_collect::<BTreeMap<_, _>>()
        .await?;
        // Store the generated delta entries as part of the sharded GitDeltaManifest
        let mut manifest = GitDeltaManifest::new(bonsai.get_changeset_id());
        // The sharded map representation might change its structure multiple times if keys are added one-by-one. Using the add_entries
        // method all manifest entries are added in one go and persisted in the blobstore
        manifest
            .add_entries(ctx, derivation_ctx.blobstore(), manifest_entries)
            .await
            .with_context(|| {
                format!(
                    "Error in storing derived data GitDeltaManifest for Bonsai changeset {}",
                    bonsai.get_changeset_id()
                )
            })?;
        // Now that the entries of the manifest are stored, store the initial manifest type itself
        let manifest_id = manifest
            .store(ctx, derivation_ctx.blobstore())
            .await
            .with_context(|| {
                format!(
                    "Error in storing GitDeltaManifest for Bonsai changeset {}",
                    bonsai.get_changeset_id()
                )
            })?;
        Ok(Self(manifest_id))
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::git_delta_manifest(
            thrift::DerivedDataGitDeltaManifest::root_git_delta_manifest_id(id),
        ) = data
        {
            GitDeltaManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::git_delta_manifest(
            thrift::DerivedDataGitDeltaManifest::root_git_delta_manifest_id(data.0.into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootGitDeltaManifestId);
