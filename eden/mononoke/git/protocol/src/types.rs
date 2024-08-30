/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::fmt::Formatter;
use std::hash::Hash;
use std::hash::Hasher;
use std::marker::Unpin;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use futures::stream::BoxStream;
use git_types::DeltaObjectKind;
use git_types::GDMV2Entry;
use git_types::GDMV2ObjectEntry;
use gix_hash::ObjectId;
use metaconfig_types::GitDeltaManifestVersion;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use packetline::encode::write_binary_packetline;
use packfile::pack::DeltaForm;
use packfile::types::PackfileItem;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use rustc_hash::FxHashSet;
use tokio::io::AsyncWrite;

use crate::Repo;

const SYMREF_HEAD: &str = "HEAD";
// The upper bound on the RSS bytes beyond which we will pause executing futures until the process
// is below the threshold. This prevents us from OOMing in case of high number of parallel clone requests
const MEMORY_BOUND: u64 = 38_000_000_000;

/// Struct representing concurrency settings used during packfile generation
#[derive(Debug, Clone, Copy)]
pub struct PackfileConcurrency {
    /// The concurrency to be used for fetching trees and blobs as part of packfile stream
    pub trees_and_blobs: usize,
    /// The concurrency to be used for fetching commits as part of packfile stream
    pub commits: usize,
    /// The concurrency to be used for fetching tags as part of packfile stream
    pub tags: usize,
    /// The upper limit on the size of process RSS allowed for streaming the packfile
    pub memory_bound: u64,
}

impl PackfileConcurrency {
    pub fn new(
        trees_and_blobs: usize,
        commits: usize,
        tags: usize,
        memory_bound: Option<u64>,
    ) -> Self {
        Self {
            trees_and_blobs,
            commits,
            tags,
            memory_bound: memory_bound.unwrap_or(MEMORY_BOUND),
        }
    }

    pub fn standard() -> Self {
        Self {
            trees_and_blobs: 18_000,
            commits: 20_000,
            tags: 20_000,
            memory_bound: MEMORY_BOUND,
        }
    }
}

/// Enum defining the type of data associated with a ref target
pub enum RefTarget {
    /// The target is a plain Git object
    Plain(ObjectId),
    /// The target is a Git object with associated metadata
    WithMetadata(ObjectId, String),
}

impl RefTarget {
    pub fn id(&self) -> &ObjectId {
        match self {
            RefTarget::Plain(oid) | RefTarget::WithMetadata(oid, _) => oid,
        }
    }

    pub fn into_object_id(self) -> ObjectId {
        match self {
            RefTarget::Plain(oid) | RefTarget::WithMetadata(oid, _) => oid,
        }
    }

    pub fn as_object_id(&self) -> &ObjectId {
        match self {
            RefTarget::Plain(oid) | RefTarget::WithMetadata(oid, _) => oid,
        }
    }

    pub fn metadata(&self) -> Option<&str> {
        match self {
            RefTarget::Plain(_) => None,
            RefTarget::WithMetadata(_, meta) => Some(meta),
        }
    }
}

impl Display for RefTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            RefTarget::Plain(oid) => write!(f, "{}", oid),
            RefTarget::WithMetadata(oid, meta) => write!(f, "{} {}", oid, meta),
        }
    }
}

/// The set of symrefs that are to be included in or excluded from the pack
#[derive(Debug, Clone, Copy)]
pub enum RequestedSymrefs {
    /// Only include the HEAD symref in the pack/bundle
    IncludeHead(SymrefFormat),
    /// Incldue all known symrefs in the pack/bundle
    IncludeAll(SymrefFormat),
    /// Exclude all known symrefs from the pack/bundle
    ExcludeAll,
}

/// The format in which the symrefs need to be included in the pack
#[derive(Debug, Clone, Copy)]
pub enum SymrefFormat {
    /// Include the symref along with the ref that it points to, e.g.
    /// object_id_here HEAD symref-target:refs/heads/master
    NameWithTarget,
    /// Only include the symref name, e.g. object_id_here HEAD
    NameOnly,
}

/// The set of refs that are to be included in or excluded from the pack
#[derive(Debug, Clone)]
pub enum RequestedRefs {
    /// Include the following refs with values known by the server
    Included(HashSet<String>),
    /// Include only those refs whose names start with the given prefix
    IncludedWithPrefix(HashSet<String>),
    /// Include the following refs with values provided by the caller
    IncludedWithValue(HashMap<String, ChangesetId>),
    /// Exclude the following refs
    Excluded(HashSet<String>),
}

impl RequestedRefs {
    pub fn all() -> Self {
        RequestedRefs::Excluded(HashSet::new())
    }

    pub fn none() -> Self {
        RequestedRefs::Included(HashSet::new())
    }
}

/// Enum defining how annotated tags should be included as a ref
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagInclusion {
    /// Peel the tag and map it to the underlying Git commit
    Peeled,
    /// Include the tag as-is without peeling and map it to
    /// the annotated Git tag object
    AsIs,
    /// Include the tag as-is without peeling but reference the
    /// peeled target of the tag
    WithTarget,
}

/// Enum defining whether a delta should be included in the pack
/// and if so, what kind of delta should be used
#[derive(Debug, Clone, Copy)]
pub enum DeltaInclusion {
    /// Include deltas with the provided form and inclusion threshold
    Include {
        /// Whether the pack input should consist of RefDeltas or only OffsetDeltas
        form: DeltaForm,
        /// The percentage threshold which should be satisfied by the delta to be included
        /// in the pack input stream. The threshold is expressed as percentage of the original (0.0 to 1.0)
        /// uncompressed object size. e.g. If original object size is 100 bytes and the
        /// delta_inclusion_threshold is 0.5, then the delta size should be less than 50 bytes
        inclusion_threshold: f32,
    },
    /// Do not include deltas
    Exclude,
}

impl DeltaInclusion {
    /// The standard delta inclusion setting used in most places
    /// in Mononoke
    pub fn standard() -> Self {
        DeltaInclusion::Include {
            form: DeltaForm::RefAndOffset,
            inclusion_threshold: 0.8,
        }
    }
}

impl DeltaInclusion {
    pub fn include_only_offset_deltas(&self) -> bool {
        match self {
            DeltaInclusion::Include { form, .. } => form == &DeltaForm::OnlyOffset,
            DeltaInclusion::Exclude => false,
        }
    }
}

/// Enum defining how packfile items for raw git objects be fetched
#[derive(clap::ValueEnum, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackfileItemInclusion {
    // Generate the packfile item for raw git object regardless of whether
    // it already exists or not. Do not store the generated packfile item.
    #[default]
    Generate,
    // Fetch the stored packfile item for the raw git object. If it doesn't
    // exist, error out
    FetchOnly,
    // If the packfile item for the raw git object already exists, use it. If
    // it doesn't exist, generate it and store it
    FetchAndStore,
}

/// Enum defining the source to be used to fetch the refs
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefsSource {
    /// Fetch the refs from the in-memory WBC. Note due to very nature of WBC
    /// these bookmarks can be stale
    #[default]
    WarmBookmarksCache,
    /// Fetch refs from the master instance of the bookmarks DB. This will always
    /// show the latest state of refs
    DatabaseMaster,
}

/// The request parameters used to specify the constraints that need to be
/// honored while generating the input PackfileItem stream
#[derive(Debug, Clone)]
pub struct PackItemStreamRequest {
    /// The symrefs that are requested to be included/excluded from the pack
    pub requested_symrefs: RequestedSymrefs,
    /// The refs that are requested to be included/excluded from the pack
    pub requested_refs: RequestedRefs,
    /// The heads of the references that are present with the client
    pub have_heads: Vec<ChangesetId>,
    /// The type of delta that should be included in the pack, if any
    pub delta_inclusion: DeltaInclusion,
    /// How annotated tags should be included in the pack
    pub tag_inclusion: TagInclusion,
    /// How packfile items for raw git objects should be fetched
    pub packfile_item_inclusion: PackfileItemInclusion,
    /// The concurrency setting to be used while generating the packfile
    pub concurrency: PackfileConcurrency,
    /// The source to be used to fetch the refs
    pub refs_source: RefsSource,
}

impl PackItemStreamRequest {
    pub fn new(
        requested_symrefs: RequestedSymrefs,
        requested_refs: RequestedRefs,
        have_heads: Vec<ChangesetId>,
        delta_inclusion: DeltaInclusion,
        tag_inclusion: TagInclusion,
        packfile_item_inclusion: PackfileItemInclusion,
    ) -> Self {
        Self {
            requested_symrefs,
            requested_refs,
            have_heads,
            delta_inclusion,
            tag_inclusion,
            packfile_item_inclusion,
            concurrency: PackfileConcurrency::standard(),
            // Packfile generation should always use the latest state of refs
            refs_source: RefsSource::DatabaseMaster,
        }
    }

    pub fn full_repo(
        delta_inclusion: DeltaInclusion,
        tag_inclusion: TagInclusion,
        packfile_item_inclusion: PackfileItemInclusion,
    ) -> Self {
        Self {
            requested_symrefs: RequestedSymrefs::IncludeHead(SymrefFormat::NameOnly),
            requested_refs: RequestedRefs::Excluded(HashSet::new()),
            have_heads: vec![],
            delta_inclusion,
            tag_inclusion,
            packfile_item_inclusion,
            concurrency: PackfileConcurrency::standard(),
            // Packfile generation should always use the latest state of refs
            refs_source: RefsSource::DatabaseMaster,
        }
    }
}

/// The request parameters used to specify the constraints that need to be
/// honored while generating the collection of refs to be sent as response to
/// ls-refs command
#[derive(Debug, Clone)]
pub struct LsRefsRequest {
    /// The symrefs that are requested to be included/excluded from the output
    pub requested_symrefs: RequestedSymrefs,
    /// The refs that are requested to be included/excluded from the output
    pub requested_refs: RequestedRefs,
    /// How annotated tags should be included in the output
    pub tag_inclusion: TagInclusion,
    /// The source to be used to fetch the refs
    pub refs_source: RefsSource,
}

impl LsRefsRequest {
    pub fn new(
        requested_symrefs: RequestedSymrefs,
        requested_refs: RequestedRefs,
        tag_inclusion: TagInclusion,
    ) -> Self {
        Self {
            requested_symrefs,
            requested_refs,
            tag_inclusion,
            refs_source: RefsSource::WarmBookmarksCache,
        }
    }

    pub fn write_advertisement() -> Self {
        Self {
            requested_symrefs: RequestedSymrefs::ExcludeAll,
            requested_refs: RequestedRefs::all(),
            tag_inclusion: TagInclusion::Peeled,
            // For write advertisement, we need to use the latest state of refs
            refs_source: RefsSource::DatabaseMaster,
        }
    }
}

/// The request parameters used to specify the constraints that need to be
/// honored while generating the packstream to be sent as response for fetch
/// command
#[derive(Debug, Clone)]
pub struct FetchRequest {
    /// Collection of commit object Ids that are requested by the client
    pub heads: Vec<ObjectId>,
    /// Collection of commit object Ids that are present with the client
    pub bases: Vec<ObjectId>,
    /// Boolean flag indicating if the packfile can contain deltas referring
    /// to objects outside the packfile
    pub include_out_of_pack_deltas: bool,
    /// Flag indicating if the packfile should contain objects corresponding to
    /// annotated tags if the commits that the tag points are present in the
    /// packfile
    pub include_annotated_tags: bool,
    /// Flag indicating if the caller supports offset deltas
    pub offset_delta: bool,
    /// Request that various objects from the packfile be omitted using
    /// one of several filtering techniques
    pub filter: Option<FetchFilter>,
    /// Information pertaining to commits that will be part of the response if the
    /// requested clone/pull is shallow
    pub shallow_info: Option<ShallowInfoResponse>,
    /// The concurrency setting to be used for generating the packfile items for the
    /// fetch request
    pub concurrency: PackfileConcurrency,
}

/// Struct representing the filtering options that can be used during fetch / clone
#[derive(Debug, Clone)]
pub struct FetchFilter {
    /// The maximum size of blob in bytes that is allowed by the client
    pub max_blob_size: u64,
    /// The maximum depth a tree OR blob can have in the packfile
    pub max_tree_depth: u64,
    /// The types of objects allowed by the client
    pub allowed_object_types: Vec<gix_object::Kind>,
}

impl FetchFilter {
    pub fn include_commits(&self) -> bool {
        self.allowed_object_types
            .contains(&gix_object::Kind::Commit)
    }

    pub fn include_tags(&self) -> bool {
        self.allowed_object_types.contains(&gix_object::Kind::Tag)
    }
}

/// Struct representing the packfile item response generated for the
/// given range of commits
pub struct PackItemStreamResponse<'a> {
    /// The stream of packfile items that were generated for the given range of commits
    pub items: BoxStream<'a, Result<PackfileItem>>,
    /// The number of packfile items that were generated for the given range of commits
    pub num_items: usize,
    /// The set of refs mapped to their Git commit ID or tag ID that are included in the
    /// generated stream of packfile items along with optional metadata for the mapping
    pub included_refs: HashMap<String, RefTarget>,
}

impl<'a> PackItemStreamResponse<'a> {
    pub fn new(
        items: BoxStream<'a, Result<PackfileItem>>,
        num_items: usize,
        included_refs: HashMap<String, RefTarget>,
    ) -> Self {
        Self {
            items,
            num_items,
            included_refs,
        }
    }
}

/// Struct representing the ls-refs response generated for the
/// given request parameters
pub struct LsRefsResponse {
    /// The set of refs mapped to their Git commit ID or tag ID that are included in the
    /// output along with optional metadata for the mapping
    pub included_refs: HashMap<String, RefTarget>,
}

pub fn ref_line(name: &str, target: &RefTarget) -> String {
    match target.metadata() {
        None => {
            format!("{} {}", target.as_object_id().to_hex(), name)
        }
        Some(metadata) => {
            format!("{} {} {}", target.as_object_id().to_hex(), name, metadata)
        }
    }
}

impl LsRefsResponse {
    pub fn new(included_refs: HashMap<String, RefTarget>) -> Self {
        Self { included_refs }
    }

    pub async fn write_packetline<W>(&self, writer: &mut W) -> Result<()>
    where
        W: AsyncWrite + Send + Unpin,
    {
        // HEAD symref should always be written first
        if let Some(target) = self.included_refs.get(SYMREF_HEAD) {
            write_binary_packetline(ref_line(SYMREF_HEAD, target).as_bytes(), writer).await?;
        }
        let mut sorted_refs = self.included_refs.iter().collect::<Vec<_>>();
        sorted_refs.sort_by(|(ref_a_name, _), (ref_b_name, _)| ref_a_name.cmp(ref_b_name));
        for (name, target) in sorted_refs {
            if name.as_str() != SYMREF_HEAD {
                write_binary_packetline(ref_line(name, target).as_bytes(), writer).await?;
            }
        }
        Ok(())
    }
}

/// Struct representing the packfile item response generated for the
/// fetch request command
pub struct FetchResponse<'a> {
    /// The stream of packfile items that were generated for the fetch request command
    pub items: BoxStream<'a, Result<PackfileItem>>,
    /// The number of packfile items that were generated for the fetch request command
    pub num_items: usize,
}

impl<'a> FetchResponse<'a> {
    pub fn new(items: BoxStream<'a, Result<PackfileItem>>, num_items: usize) -> Self {
        Self { items, num_items }
    }
}

/// Enum representing the type of shallow clone/fetch that is requested by the client
#[derive(Debug, Clone)]
pub enum ShallowVariant {
    /// The fetch/clone requested by the client has no shallow properties
    None,
    /// Requests that the fetch/clone should be shallow having a commit
    /// depth of "deepen" relative to the server
    FromServerWithDepth(u32),
    /// Requests that the semantics of the "deepen" command be changed
    /// to indicate that the depth requested is relative to the client's
    /// current shallow boundary, instead of relative to the requested commits.
    FromClientWithDepth(u32),
    /// Requests that the shallow clone/fetch should be cut at a specific time,
    /// instead of depth. The timestamp provided should be in the same format
    /// as is expected for git rev-list --max-age <timestamp>
    FromServerWithTime(gix_date::Time),
    /// Requests that the shallow clone/fetch should be cut at a specific revision
    /// instead of a depth, i.e. the specified oid becomes the boundary at which the
    /// fetch or clone should stop at
    FromServerWithOid(ObjectId),
}

impl ShallowVariant {
    pub fn is_none(&self) -> bool {
        matches!(self, ShallowVariant::None)
    }
}

/// Struct representing the request parameters for shallow info section in Git fetch response
#[derive(Debug, Clone)]
pub struct ShallowInfoRequest {
    /// List of commit object Ids that are requested by the client
    pub heads: Vec<ObjectId>,
    /// List of object Ids representing the edge of the shallow history present
    /// at the client, i.e. the set of commits that the client knows about but
    /// does not have any of their parents and their ancestors
    pub shallow: Vec<ObjectId>,
    /// The type of shallow clone/fetch that is requested by the client
    pub variant: ShallowVariant,
}

impl ShallowInfoRequest {
    pub fn shallow_requested(&self) -> bool {
        !self.variant.is_none()
    }
}

/// Struct representing the response for shallow info section in Git fetch response
#[derive(Debug, Clone)]
pub struct ShallowInfoResponse {
    /// The set of commits that need to be returned as part of the shallow clone/fetch
    pub commits: Vec<ChangesetId>,
    /// The set of commits that are returned as part of the shallow clone/fetch but also
    /// form the boundary of the shallow history sent by the server
    pub boundary_commits: Vec<ChangesetId>,
    /// The set of commits that are considered as shallow at the client
    pub client_shallow: Vec<ChangesetId>,
}

impl ShallowInfoResponse {
    pub fn new(
        commits: Vec<ChangesetId>,
        boundary_commits: Vec<ChangesetId>,
        client_shallow: Vec<ChangesetId>,
    ) -> Self {
        Self {
            commits,
            boundary_commits,
            client_shallow,
        }
    }

    /// Method responsible for fetching the commits that must be unshallowed by the
    /// client
    pub fn client_unshallow_commits(&self) -> Vec<ChangesetId> {
        self.client_shallow
            .iter()
            .filter(|shallow_commit| self.commits.contains(shallow_commit))
            .copied()
            .collect()
    }
}

/// Struct representing a complete Git content object (tree or blob) entry
/// that is expressed without any delta
#[derive(Debug, Clone)]
pub(crate) struct FullObjectEntry {
    pub(crate) cs_id: ChangesetId,
    pub(crate) path: MPath,
    pub(crate) oid: ObjectId,
    pub(crate) rich_git_sha: RichGitSha1,
}

impl FullObjectEntry {
    pub fn new(cs_id: ChangesetId, path: MPath, rich_git_sha: RichGitSha1) -> Result<Self> {
        let oid = rich_git_sha.sha1().to_object_id()?;
        Ok(Self {
            cs_id,
            path,
            oid,
            rich_git_sha,
        })
    }

    pub fn into_delta_manifest_entry(self) -> GDMV2Entry {
        let size = self.rich_git_sha.size();
        let kind = if self.rich_git_sha.is_blob() {
            DeltaObjectKind::Blob
        } else {
            DeltaObjectKind::Tree
        };
        GDMV2Entry {
            full_object: GDMV2ObjectEntry {
                size,
                kind,
                oid: self.oid,
                inlined_bytes: None,
            },
            deltas: vec![],
        }
    }
}

impl Hash for FullObjectEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rich_git_sha.hash(state);
    }
}

impl PartialEq for FullObjectEntry {
    fn eq(&self, other: &Self) -> bool {
        self.rich_git_sha == other.rich_git_sha
    }
}

impl Eq for FullObjectEntry {}

/// Set of parameters that are needed by the generators used for constructing
/// response for fetch request
#[derive(Clone)]
pub(crate) struct FetchContainer {
    pub(crate) ctx: Arc<CoreContext>,
    pub(crate) blobstore: Arc<RepoBlobstore>,
    pub(crate) derived_data: Arc<RepoDerivedData>,
    pub(crate) git_delta_manifest_version: GitDeltaManifestVersion,
    pub(crate) delta_inclusion: DeltaInclusion,
    pub(crate) filter: Arc<Option<FetchFilter>>,
    pub(crate) concurrency: PackfileConcurrency,
    pub(crate) packfile_item_inclusion: PackfileItemInclusion,
    pub(crate) shallow_info: Arc<Option<ShallowInfoResponse>>,
}

impl FetchContainer {
    pub(crate) fn new(
        ctx: Arc<CoreContext>,
        repo: &impl Repo,
        delta_inclusion: DeltaInclusion,
        filter: Arc<Option<FetchFilter>>,
        concurrency: PackfileConcurrency,
        packfile_item_inclusion: PackfileItemInclusion,
        shallow_info: Arc<Option<ShallowInfoResponse>>,
    ) -> Result<Self> {
        let git_delta_manifest_version = repo
            .repo_config()
            .derived_data_config
            .get_active_config()
            .ok_or_else(|| anyhow!("No enabled derived data types config"))?
            .git_delta_manifest_version;
        Ok(Self {
            ctx,
            git_delta_manifest_version,
            delta_inclusion,
            filter,
            concurrency,
            packfile_item_inclusion,
            shallow_info,
            blobstore: repo.repo_blobstore_arc(),
            derived_data: repo.repo_derived_data_arc(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CommitTagMappings {
    pub(crate) tagged_commits: Vec<ChangesetId>,
    pub(crate) tag_names: Arc<FxHashSet<String>>,
    pub(crate) non_tag_oids: Vec<ObjectId>,
}

#[derive(Debug, Clone)]
pub(crate) struct TranslatedShas {
    pub(crate) bonsais: Vec<ChangesetId>,
    pub(crate) tag_names: Arc<FxHashSet<String>>,
    pub(crate) non_tag_non_commit_oids: Vec<ObjectId>,
}

impl TranslatedShas {
    pub(crate) fn new(mut commit_bonsais: Vec<ChangesetId>, mappings: CommitTagMappings) -> Self {
        commit_bonsais.extend(mappings.tagged_commits);
        Self {
            bonsais: commit_bonsais,
            tag_names: mappings.tag_names,
            non_tag_non_commit_oids: mappings.non_tag_oids,
        }
    }
}
