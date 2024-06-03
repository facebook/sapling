/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks_cache::BookmarksCacheRef;
use buffered_weighted::StreamExt as _;
use bytes::Bytes;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use commit_graph_types::frontier::AncestorsWithinDistance;
use context::CoreContext;
use futures::future;
use futures::future::Either;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt as _;
use futures::TryStreamExt;
use git_symbolic_refs::GitSymbolicRefsRef;
use git_types::fetch_git_delta_manifest;
use git_types::fetch_git_object_bytes;
use git_types::fetch_non_blob_git_object_bytes;
use git_types::fetch_packfile_base_item;
use git_types::fetch_packfile_base_item_if_exists;
use git_types::mode;
use git_types::upload_packfile_base_item;
use git_types::DeltaObjectKind;
use git_types::GitDeltaManifestEntryOps;
use git_types::GitDeltaManifestOps;
use git_types::GitIdentifier;
use git_types::HeaderState;
use git_types::ObjectDeltaOps;
use git_types::TreeHandle;
use git_types::TreeMember;
use gix_hash::ObjectId;
use manifest::ManifestOps;
use metaconfig_types::GitDeltaManifestVersion;
use metaconfig_types::RepoConfigRef;
use mononoke_types::hash::GitSha1;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use packfile::types::PackfileItem;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::types::DeltaInclusion;
use crate::types::FetchFilter;
use crate::types::FetchRequest;
use crate::types::FetchResponse;
use crate::types::FullObjectEntry;
use crate::types::LsRefsRequest;
use crate::types::LsRefsResponse;
use crate::types::PackItemStreamRequest;
use crate::types::PackItemStreamResponse;
use crate::types::PackfileConcurrency;
use crate::types::PackfileItemInclusion;
use crate::types::RefTarget;
use crate::types::RequestedRefs;
use crate::types::RequestedSymrefs;
use crate::types::ShallowInfoRequest;
use crate::types::ShallowInfoResponse;
use crate::types::ShallowVariant;
use crate::types::SymrefFormat;
use crate::types::TagInclusion;

const HEAD_REF: &str = "HEAD";
const TAGS_PREFIX: &str = "tags/";
const REF_PREFIX: &str = "refs/";

// The threshold in bytes below which we consider a future cheap enough to have a weight of 1
const THRESHOLD_BYTES: usize = 6000;

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + RepoDerivedDataArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + RepoDerivedDataRef
    + GitSymbolicRefsRef
    + CommitGraphRef
    + BookmarksCacheRef
    + RepoConfigRef
    + Send
    + Sync;

/// Set of parameters that are needed by the generators used for constructing
/// response for fetch request
#[derive(Clone)]
struct FetchContainer {
    ctx: Arc<CoreContext>,
    blobstore: Arc<RepoBlobstore>,
    derived_data: Arc<RepoDerivedData>,
    git_delta_manifest_version: GitDeltaManifestVersion,
    delta_inclusion: DeltaInclusion,
    filter: Arc<Option<FetchFilter>>,
    concurrency: PackfileConcurrency,
    packfile_item_inclusion: PackfileItemInclusion,
    shallow_info: Arc<Option<ShallowInfoResponse>>,
}

impl FetchContainer {
    fn new(
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

/// Get the bookmarks (branches, tags) and their corresponding commits
/// for the given repo based on the request parameters. If the request
/// specifies a predefined mapping of an existing or new bookmark to a
/// commit, include that in the output as well
async fn bookmarks(
    ctx: &CoreContext,
    repo: &impl Repo,
    requested_refs: &RequestedRefs,
) -> Result<FxHashMap<BookmarkKey, ChangesetId>> {
    let mut bookmarks = repo
        .bookmarks_cache()
        .list(
            ctx,
            &BookmarkPrefix::empty(),
            &BookmarkPagination::FromStart,
            None, // Limit
        )
        .await?
        .into_iter()
        .filter_map(|(bookmark, (cs_id, _))| {
            let refs = requested_refs.clone();
            let name = bookmark.name().to_string();
            match refs {
                RequestedRefs::Included(refs) if refs.contains(&name) => Some((bookmark, cs_id)),
                RequestedRefs::IncludedWithPrefix(ref_prefixes) => {
                    let ref_name = format!("{}{}", REF_PREFIX, name);
                    if ref_prefixes
                        .iter()
                        .any(|ref_prefix| ref_name.starts_with(ref_prefix))
                    {
                        Some((bookmark, cs_id))
                    } else {
                        None
                    }
                }
                RequestedRefs::Excluded(refs) if !refs.contains(&name) => Some((bookmark, cs_id)),
                RequestedRefs::IncludedWithValue(refs) => {
                    refs.get(&name).map(|cs_id| (bookmark, cs_id.clone()))
                }
                _ => None,
            }
        })
        .collect::<FxHashMap<_, _>>();
    // In case the requested refs include specified refs with value and those refs are not
    // bookmarks known at the server, we need to manually include them in the output
    if let RequestedRefs::IncludedWithValue(ref ref_value_map) = requested_refs {
        for (ref_name, ref_value) in ref_value_map {
            bookmarks.insert(
                BookmarkKey::with_name(ref_name.as_str().try_into()?),
                ref_value.clone(),
            );
        }
    }
    Ok(bookmarks)
}

/// Get the refs (branches, tags) and their corresponding object ids
/// The input refs should be of the form `refs/<ref_name>`
pub async fn ref_oid_mapping(
    ctx: &CoreContext,
    repo: &impl Repo,
    requested_refs: impl IntoIterator<Item = String>,
) -> Result<impl Iterator<Item = (String, ObjectId)>> {
    let requested_refs = RequestedRefs::Included(
        requested_refs
            .into_iter()
            .map(|want_ref| want_ref.trim_start_matches(REF_PREFIX).to_owned())
            .collect(),
    );
    let wanted_refs = bookmarks(ctx, repo, &requested_refs)
        .await
        .context("Error while fetching bookmarks for ref_oid_mapping")?;
    let bonsai_git_mappings =
        bonsai_git_mappings_by_bonsai(ctx, repo, wanted_refs.values().copied().collect())
            .await
            .context("Error while fetching bonsai_git_mapping for ref_oid_mapping")?;
    let wanted_refs_with_oid = wanted_refs
        .into_iter()
        .map(|(bookmark, cs_id)| {
            let oid = bonsai_git_mappings.get(&cs_id).with_context(|| {
                format!(
                    "Error while fetching git sha1 for bonsai commit {} in ref_oid_mapping",
                    cs_id
                )
            })?;
            let ref_name = format!("{}{}", REF_PREFIX, bookmark.name());
            Ok((ref_name, oid.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(wanted_refs_with_oid.into_iter())
}

/// Function determining if the current object entry at the given path should be
/// filtered in the resultant packfile
fn filter_object(
    filter: Arc<Option<FetchFilter>>,
    path: &MPath,
    kind: DeltaObjectKind,
    size: u64,
) -> bool {
    match filter.as_ref() {
        Some(filter) => {
            let too_deep =
                (kind.is_tree() || kind.is_blob()) && path.depth() >= filter.max_tree_depth;
            let too_large = kind.is_blob() && size >= filter.max_blob_size;
            let invalid_type = !filter.allowed_object_types.contains(&kind.to_gix_kind());
            // The object passes the filter if its not too deep and not too large and its type is allowed
            !too_deep && !too_large && !invalid_type
        }
        // If there is no filter, then we should not exclude any objects
        None => true,
    }
}

/// Fetch and collect the tree and blob objects that are expressed as full objects
/// for the boundary commits of a shallow fetch
async fn boundary_trees_and_blobs(
    fetch_container: FetchContainer,
) -> Result<FxHashSet<FullObjectEntry>> {
    let FetchContainer {
        ctx,
        derived_data,
        blobstore,
        filter,
        concurrency,
        shallow_info,
        ..
    } = fetch_container;
    let boundary_commits = match shallow_info.as_ref() {
        Some(shallow_info) => shallow_info.boundary_commits.clone(),
        None => Vec::new(),
    };
    stream::iter(boundary_commits.into_iter().map(Ok))
        .map_ok(|changeset_id| {
            cloned!(ctx, derived_data, blobstore, filter);
            async move {
                let root_tree = derived_data
                    .derive::<TreeHandle>(&ctx, changeset_id)
                    .await
                    .with_context(|| {
                        format!(
                            "Error in deriving TreeHandle for changeset {:?}",
                            changeset_id
                        )
                    })?;
                let objects = root_tree
                    .list_all_entries((*ctx).clone(), blobstore)
                    .try_filter_map(|(path, entry)| {
                        let filter = filter.clone();
                        let tree_member = TreeMember::from(entry);
                        let kind = if tree_member.oid().is_blob() {
                            DeltaObjectKind::Blob
                        } else {
                            DeltaObjectKind::Tree
                        };
                        async move {
                            // If the entry corresponds to a submodules (and shows up as a commit), then we ignore it
                            let is_submodule = tree_member.filemode() == mode::GIT_FILEMODE_COMMIT;
                            // If the object is ignored by the filter, then we ignore it
                            if !filter_object(filter, &path, kind, tree_member.oid().size()) || is_submodule {
                                Ok(None)
                            } else {
                                Ok(Some(FullObjectEntry::new(changeset_id, path, *tree_member.oid())?))
                            }
                        }
                    })
                    .try_collect::<FxHashSet<_>>()
                    .await
                    .with_context(|| {
                        format!(
                            "Error while listing all entries from TreeHandle for changeset {changeset_id:?}",
                        )
                    })?;
                Ok(objects)
            }
        })
        .try_buffered(concurrency.commits)
        .try_concat()
        .await
}

/// Get the count of distinct blob and tree items to be included in the packfile along with the
/// set of base objects that are expected to be present at the client
async fn trees_and_blobs_count(
    fetch_container: FetchContainer,
    target_commits: BoxStream<'_, Result<ChangesetId>>,
) -> Result<(usize, FxHashSet<ObjectId>)> {
    let FetchContainer {
        ctx,
        git_delta_manifest_version,
        delta_inclusion,
        derived_data,
        blobstore,
        filter,
        concurrency,
        ..
    } = fetch_container.clone();
    let boundary_stream = stream::once(async move {
        boundary_trees_and_blobs(fetch_container)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|full_entry| {
                        let empty_base: Option<ObjectId> = None;
                        (full_entry.oid, empty_base)
                    })
                    .collect::<Vec<_>>()
            })
    });
    // Sum up the entries in the delta manifest for each commit included in packfile
    let body_stream = target_commits
        .map_ok(|changeset_id| {
            cloned!(ctx, derived_data, blobstore, filter);
            async move {
                let delta_manifest = fetch_git_delta_manifest(
                    &ctx,
                    &derived_data,
                    &blobstore,
                    git_delta_manifest_version,
                    changeset_id,
                )
                .await?;
                // Get the FxHashSet of the tree and blob object Ids that will be included
                // in the packfile
                let objects = delta_manifest
                    .into_subentries(&ctx, &blobstore)
                    .try_filter_map(|(path, entry)| {
                        cloned!(filter);
                        async move {
                            let (kind, size) = (entry.full_object_kind(), entry.full_object_size());
                            // If the entry does not pass the filter, then it should not be included in the count
                            if !filter_object(filter.clone(), &path, kind, size) {
                                return Ok(None);
                            }
                            let delta = delta_base(&entry, delta_inclusion, filter);
                            let output = (
                                entry.full_object_oid(),
                                delta.map(|delta| delta.base_object_oid()),
                            );
                            Ok(Some(output))
                        }
                    })
                    .try_collect::<Vec<_>>()
                    .await
                    .with_context(|| {
                        format!(
                            "Error while listing entries from GitDeltaManifest for changeset {:?}",
                            changeset_id,
                        )
                    })?;
                Ok(objects)
            }
        })
        .try_buffered(concurrency.trees_and_blobs);

    boundary_stream
        .chain(body_stream)
        .try_fold(
            (FxHashSet::default(), FxHashSet::default()),
            |(mut object_set, mut base_set), objects_with_bases| async move {
                for (object, base) in objects_with_bases {
                    // If the object is already used as a base, then it should NOT be
                    // part of the packfile
                    if !base_set.contains(&object) {
                        object_set.insert(object);
                        if let Some(base_oid) = base {
                            // If the base of this delta was already counted as part of the packfile,
                            // then do NOT add it to the set of base objects
                            if !object_set.contains(&base_oid) {
                                base_set.insert(base_oid);
                            }
                        }
                    }
                }
                Ok((object_set, base_set))
            },
        )
        .await
        .map(|(object_set, base_set)| (object_set.len(), base_set))
}

fn delta_below_threshold(
    delta: &impl ObjectDeltaOps,
    full_object_size: u64,
    inclusion_threshold: f32,
) -> bool {
    (delta.instructions_compressed_size() as f64)
        < (full_object_size as f64) * inclusion_threshold as f64
}

fn delta_base(
    entry: &impl GitDeltaManifestEntryOps,
    delta_inclusion: DeltaInclusion,
    filter: Arc<Option<FetchFilter>>,
) -> Option<impl ObjectDeltaOps + Send> {
    match delta_inclusion {
        DeltaInclusion::Include {
            inclusion_threshold,
            ..
        } => entry
            .deltas()
            .min_by(|a, b| {
                a.instructions_compressed_size()
                    .cmp(&b.instructions_compressed_size())
            })
            .filter(|delta| {
                let path = delta.base_object_path();
                let kind = delta.base_object_kind();
                let size = delta.base_object_size();
                // Is the delta defined in terms of itself (i.e. A as delta of A)? If yes, then we
                // should use the full object to avoid cycle
                let is_self_delta = delta.base_object_oid() == entry.full_object_oid();
                // Only use the delta if it is below the threshold and passes the filter
                delta_below_threshold(*delta, entry.full_object_size(), inclusion_threshold)
                    && filter_object(filter, path, kind, size)
                    && !is_self_delta
            })
            .cloned(),
        // Can't use the delta variant if the request prevents us from using it
        DeltaInclusion::Exclude => None,
    }
}

fn to_commit_stream(commits: Vec<ChangesetId>) -> BoxStream<'static, Result<ChangesetId>> {
    stream::iter(commits.into_iter().map(Ok)).boxed()
}

async fn commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    heads: Vec<ChangesetId>,
    bases: Vec<ChangesetId>,
    shallow_info: &Option<ShallowInfoResponse>,
) -> Result<Vec<ChangesetId>> {
    match shallow_info {
        Some(shallow_info) => Ok(shallow_info.commits.clone()),
        None => {
            repo.commit_graph()
                .ancestors_difference_stream(ctx, heads, bases)
                .await
                .context("Error in getting stream of commits between heads and bases during fetch")?
                .try_collect::<Vec<_>>()
                .await
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CommitTagMappings {
    tagged_commits: Vec<ChangesetId>,
    tag_names: Arc<FxHashSet<String>>,
    non_tag_oids: Vec<ObjectId>,
}

/// Fetch all the bonsai commits pointed to by the annotated tags corresponding
/// to the input object ids along with the tag names. For all the input Git shas
/// that we could not find a corresponding tag for, return the shas as blob and tree
/// objects
async fn tagged_commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    git_shas: Vec<GitSha1>,
) -> Result<CommitTagMappings> {
    if git_shas.is_empty() {
        return Ok(CommitTagMappings::default());
    }
    let mut non_tag_shas = git_shas.iter().cloned().collect::<FxHashSet<GitSha1>>();
    // Fetch the names of the tags corresponding to the tag object represented by the input object ids
    let tag_names = repo
        .bonsai_tag_mapping()
        .get_entries_by_tag_hashes(git_shas)
        .await
        .context("Error while fetching tag entries from tag hashes")?
        .into_iter()
        .map(|entry| {
            non_tag_shas.remove(&entry.tag_hash);
            entry.tag_name
        })
        .collect::<FxHashSet<String>>();
    let tag_names = Arc::new(tag_names);
    // Fetch the commits pointed to by those tags
    // TODO: We can probably do the filtering on the DB instead of on the server
    let tagged_commits = repo
        .bookmarks_cache()
        .list(
            ctx,
            &BookmarkPrefix::new(TAGS_PREFIX)?,
            &BookmarkPagination::FromStart,
            None, // Limit
        )
        .await
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|(bookmark, (cs_id, _))| {
                    if tag_names.contains(&bookmark.name().to_string()) {
                        Some(cs_id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })?;
    let non_tag_oids = non_tag_shas
        .into_iter()
        .map(|sha| sha.to_object_id())
        .collect::<Result<Vec<_>>>()
        .context("Error in converting non-tag shas to object ids")?;
    Ok(CommitTagMappings {
        tagged_commits,
        tag_names,
        non_tag_oids,
    })
}

#[derive(Debug, Clone)]
struct TranslatedShas {
    bonsais: Vec<ChangesetId>,
    tag_names: Arc<FxHashSet<String>>,
    non_tag_non_commit_oids: Vec<ObjectId>,
}

impl TranslatedShas {
    fn new(mut commit_bonsais: Vec<ChangesetId>, mappings: CommitTagMappings) -> Self {
        commit_bonsais.extend(mappings.tagged_commits);
        Self {
            bonsais: commit_bonsais,
            tag_names: mappings.tag_names,
            non_tag_non_commit_oids: mappings.non_tag_oids,
        }
    }
}

/// Fetch the corresponding bonsai commits for the input Git object ids. If the object id doesn't
/// correspond to a bonsai commit, try to resolve it to a tag and then fetch the bonsai commit and
/// return it along with the tag name
async fn git_shas_to_bonsais(
    ctx: &CoreContext,
    repo: &impl Repo,
    oids: impl Iterator<Item = impl AsRef<gix_hash::oid>>,
) -> Result<TranslatedShas> {
    let shas = oids
        .map(|oid| GitSha1::from_object_id(oid.as_ref()))
        .collect::<Result<Vec<_>>>()
        .context("Error while converting Git object Ids to Git Sha1 during fetch")?;
    // Get the bonsai commits corresponding to the Git shas
    let entries = repo
        .bonsai_git_mapping()
        .get(ctx, BonsaisOrGitShas::GitSha1(shas.clone()))
        .await
        .with_context(|| {
            format!(
                "Failed to fetch bonsai_git_mapping for repo {}",
                repo.repo_identity().name()
            )
        })?;
    // Filter out the git shas for which we don't have an entry in the bonsai_git_mapping table
    // These are likely annotated tags which need to be resolved separately
    let tag_shas = shas
        .into_iter()
        .filter(|&sha| !entries.iter().any(|entry| entry.git_sha1 == sha))
        .collect::<Vec<_>>();
    let commit_tag_mappings = tagged_commits(ctx, repo, tag_shas)
        .await
        .context("Error while resolving annotated tags to their commits")?;
    Ok(TranslatedShas::new(
        entries.into_iter().map(|entry| entry.bcs_id).collect(),
        commit_tag_mappings,
    ))
}

/// Fetch the Bonsai Git Mappings for the given bonsais
pub async fn bonsai_git_mappings_by_bonsai(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_ids: Vec<ChangesetId>,
) -> Result<FxHashMap<ChangesetId, ObjectId>> {
    // Get the Git shas corresponding to the Bonsai commits
    repo.bonsai_git_mapping()
        .get(ctx, BonsaisOrGitShas::Bonsai(cs_ids))
        .await
        .with_context(|| {
            format!(
                "Failed to fetch bonsai_git_mapping for repo {}",
                repo.repo_identity().name()
            )
        })?
        .into_iter()
        .map(|entry| Ok((entry.bcs_id, entry.git_sha1.to_object_id()?)))
        .collect::<Result<FxHashMap<_, _>>>()
        .context("Error while converting Git Sha1 to Git Object Id during fetch")
}

/// Get the list of Git refs that need to be included in the stream of PackfileItem. On Mononoke end, this
/// will be bookmarks created from branches and tags. Branches and simple tags will be mapped to the
/// Git commit that they point to. Annotated tags will be handled based on the `tag_inclusion` parameter
async fn refs_to_include(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    tag_inclusion: TagInclusion,
) -> Result<FxHashMap<String, RefTarget>> {
    let bonsai_git_map =
        bonsai_git_mappings_by_bonsai(ctx, repo, bookmarks.values().cloned().collect()).await?;
    let bonsai_tag_map = repo
        .bonsai_tag_mapping()
        .get_all_entries()
        .await
        .with_context(|| {
            format!(
                "Error while fetching tag entries for repo {}",
                repo.repo_identity().name()
            )
        })?
        .into_iter()
        .map(|entry| Ok((entry.tag_name, entry.tag_hash.to_object_id()?)))
        .collect::<Result<FxHashMap<_, _>>>()?;

    bookmarks.iter().map(|(bookmark, cs_id)| {
        if bookmark.is_tag() {
            match tag_inclusion {
                TagInclusion::AsIs => {
                    if let Some(git_objectid) = bonsai_tag_map.get(&bookmark.to_string()) {
                        let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                        return Ok((ref_name, RefTarget::Plain(git_objectid.clone())));
                    }
                }
                TagInclusion::Peeled => {
                    let git_objectid = bonsai_git_map.get(cs_id).ok_or_else(|| {
                        anyhow::anyhow!("No Git ObjectId found for changeset {:?} during refs-to-include", cs_id)
                    })?;
                    let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                    return Ok((ref_name, RefTarget::Plain(git_objectid.clone())));
                }
                TagInclusion::WithTarget => {
                    if let Some(tag_objectid) = bonsai_tag_map.get(&bookmark.to_string()) {
                        let commit_objectid = bonsai_git_map.get(cs_id).ok_or_else(|| {
                            anyhow::anyhow!("No Git ObjectId found for changeset {:?} during refs-to-include", cs_id)
                        })?;
                        let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                        let metadata = format!("peeled:{}", commit_objectid.to_hex());
                        return Ok((
                            ref_name,
                            RefTarget::WithMetadata(tag_objectid.clone(), metadata),
                        ));
                    }
                }
            }
        };
        // If the bookmark is a branch or if its just a simple (non-annotated) tag, we generate the
        // ref to target mapping based on the changeset id
        let git_objectid = bonsai_git_map.get(cs_id).ok_or_else(|| {
            anyhow::anyhow!("No Git ObjectId found for changeset {:?} during refs-to-include", cs_id)
        })?;
        let ref_name = format!("{}{}", REF_PREFIX, bookmark);
        Ok((ref_name, RefTarget::Plain(git_objectid.clone())))
    })
    .collect::<Result<FxHashMap<_, _>>>()
}

/// Generate the appropriate RefTarget for symref based on the symref format
fn symref_target(
    symref_target: &str,
    commit_id: ObjectId,
    symref_format: SymrefFormat,
) -> RefTarget {
    match symref_format {
        SymrefFormat::NameWithTarget => {
            let metadata = format!("symref-target:{}", symref_target);
            RefTarget::WithMetadata(commit_id, metadata)
        }
        SymrefFormat::NameOnly => RefTarget::Plain(commit_id),
    }
}

/// The HEAD ref in Git doesn't have a direct counterpart in Mononoke bookmarks and is instead
/// stored in the git_symbolic_refs. Fetch the mapping and add them to the list of refs to include
async fn include_symrefs(
    repo: &impl Repo,
    requested_symrefs: RequestedSymrefs,
    refs_to_include: &mut FxHashMap<String, RefTarget>,
) -> Result<()> {
    let symref_commit_mapping = match requested_symrefs {
        RequestedSymrefs::IncludeHead(symref_format) => {
            // Get the branch that the HEAD symref points to
            let head_ref = repo
                .git_symbolic_refs()
                .get_ref_by_symref(HEAD_REF.to_string())
                .await
                .with_context(|| {
                    format!(
                        "Error in getting HEAD reference for repo {:?}",
                        repo.repo_identity().name()
                    )
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HEAD reference not found for repo {:?}",
                        repo.repo_identity().name()
                    )
                })?;
            // Get the commit id pointed by the HEAD reference
            let head_commit_id = refs_to_include
                .get(&head_ref.ref_name_with_type())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HEAD reference points to branch/tag {} which does not exist. Known refs: {:?}",
                        &head_ref.ref_name_with_type(),
                        refs_to_include.keys()
                    )
                })?
                .id();
            let ref_target = symref_target(
                &head_ref.ref_name_with_type(),
                head_commit_id.clone(),
                symref_format,
            );
            FxHashMap::from_iter([(head_ref.symref_name, ref_target)])
        }
        RequestedSymrefs::IncludeAll(symref_format) => {
            // Get all the symrefs with the branches/tags that they point to
            let symref_entries = repo
                .git_symbolic_refs()
                .list_all_symrefs()
                .await
                .with_context(|| {
                    format!(
                        "Error in getting all symrefs for repo {:?}",
                        repo.repo_identity().name()
                    )
                })?;
            // Get the commit ids pointed by each symref
            symref_entries.into_iter().map(|entry| {
                let ref_commit_id = refs_to_include
                    .get(&entry.ref_name_with_type())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{} reference points to branch/tag {} which does not exist. Known refs: {:?}",
                            &entry.symref_name,
                            &entry.ref_name_with_type(),
                            refs_to_include.keys()
                        )
                    })?
                    .id();
                let ref_target = symref_target(&entry.ref_name_with_type(), ref_commit_id.clone(), symref_format);
                Ok((entry.symref_name, ref_target))
            }).collect::<Result<FxHashMap<_, _>>>()?
        }
        RequestedSymrefs::ExcludeAll => FxHashMap::default(),
    };

    // Add the symref -> commit mapping to the refs_to_include map
    refs_to_include.extend(symref_commit_mapping.into_iter());
    Ok(())
}

/// The type of identifier used for identifying the base git object
/// for fetching from the blobstore
enum ObjectIdentifierType {
    /// A GitIdentifier hash has information about the type and size of the object
    /// and hence can be used as an identifier for all types of Git objects
    AllObjects(GitIdentifier),
    /// The ObjectId cannot provide type and size information and hence should be
    /// used only when the object is NOT a blob
    NonBlobObjects(ObjectId),
}

impl ObjectIdentifierType {
    pub fn to_object_id(&self) -> Result<ObjectId> {
        match self {
            Self::AllObjects(ident) => match ident {
                GitIdentifier::Basic(sha) => sha.to_object_id(),
                GitIdentifier::Rich(rich_sha) => rich_sha.to_object_id(),
            },
            Self::NonBlobObjects(oid) => Ok(*oid),
        }
    }
}

/// Fetch the raw content of the Git object based on the type of identifier provided
async fn object_bytes(
    ctx: &CoreContext,
    blobstore: ArcRepoBlobstore,
    id: ObjectIdentifierType,
) -> Result<Bytes> {
    let bytes = match id {
        ObjectIdentifierType::AllObjects(git_ident) => {
            // The object identifier has been passed along with size and type information. This means
            // that it can be any type of Git object. We store Git blobs as file content and all other
            // Git objects as raw git content. The fetch_git_object_bytes function fetches from the appropriate
            // source depending on the type of the object.
            fetch_git_object_bytes(ctx, blobstore.clone(), &git_ident, HeaderState::Included)
                .await?
        }
        ObjectIdentifierType::NonBlobObjects(oid) => {
            // The object identifier has only been passed with an ObjectId. This means that it must be a
            // non-blob Git object that can be fetched directly from the blobstore.
            fetch_non_blob_git_object_bytes(ctx, &blobstore, oid.as_ref()).await?
        }
    };
    Ok(bytes)
}

/// Fetch (or generate and fetch) the packfile item for the base git object
/// based on the packfile_item_inclusion setting
async fn base_packfile_item(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    id: ObjectIdentifierType,
    packfile_item_inclusion: PackfileItemInclusion,
) -> Result<PackfileItem> {
    let git_objectid = id.to_object_id()?;
    match packfile_item_inclusion {
        // Generate the packfile item based on the raw commit object
        PackfileItemInclusion::Generate => {
            let object_bytes = object_bytes(&ctx, blobstore.clone(), id).await.with_context(|| {
                format!(
                    "Error in fetching raw git object bytes for object {:?} while generating packfile item",
                    &git_objectid
                )
            })?;
            let packfile_item = PackfileItem::new_base(object_bytes).with_context(|| {
                format!(
                    "Error in creating packfile item from git object bytes for {:?}",
                    &git_objectid
                )
            })?;
            Ok(packfile_item)
        }
        // Return the stored packfile item if it exists, otherwise error out
        PackfileItemInclusion::FetchOnly => {
            let packfile_base_item =
                fetch_packfile_base_item(&ctx, &blobstore, git_objectid.as_ref())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in fetching packfile item for git object {:?} in FetchOnly mode",
                            &git_objectid
                        )
                    })?;
            Ok(PackfileItem::new_encoded_base(
                packfile_base_item.try_into()?,
            ))
        }
        // Return the stored packfile item if its exists, if it doesn't exist, generate it and store it
        PackfileItemInclusion::FetchAndStore => {
            let fetch_result = fetch_packfile_base_item_if_exists(
                &ctx,
                &blobstore,
                git_objectid.as_ref(),
            )
            .await
            .with_context(|| {
                format!(
                    "Error in fetching packfile item for git object {:?} in FetchAndStore mode",
                    &git_objectid
                )
            })?;
            match fetch_result {
                Some(packfile_base_item) => Ok(PackfileItem::new_encoded_base(
                    packfile_base_item.try_into()?,
                )),
                None => {
                    let object_bytes = object_bytes(&ctx, blobstore.clone(), id).await.with_context(|| {
                        format!(
                            "Error in fetching raw git object bytes for object {:?} while fetching-and-storing packfile item",
                            &git_objectid
                        )
                    })?;
                    let packfile_base_item = upload_packfile_base_item(
                        &ctx,
                        &blobstore,
                        git_objectid.as_ref(),
                        object_bytes.to_vec(),
                    )
                    .await?;
                    Ok(PackfileItem::new_encoded_base(
                        packfile_base_item.try_into()?,
                    ))
                }
            }
        }
    }
}

/// Fetch the stream of blob and tree objects as delta manifest entries for the given changeset
async fn tree_and_blob_packfile_items(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    derived_data: ArcRepoDerivedData,
    git_delta_manifest_version: GitDeltaManifestVersion,
    changeset_id: ChangesetId,
) -> Result<BoxStream<'static, Result<(ChangesetId, MPath, impl GitDeltaManifestEntryOps)>>> {
    let delta_manifest = fetch_git_delta_manifest(
        &ctx,
        &derived_data,
        &blobstore,
        git_delta_manifest_version,
        changeset_id,
    )
    .await?;
    // Most delta manifests would contain tens of entries. These entries are just metadata and
    // not the actual object so its safe to load them all into memory instead of chaining streams
    // which significantly slows down the entire process.
    let entries = delta_manifest
        .into_subentries(&ctx, &blobstore)
        .map_ok(|(path, entry)| (changeset_id, path, entry))
        .try_collect::<Vec<_>>()
        .await?;
    Ok(stream::iter(entries.into_iter().map(Ok)).boxed())
}

async fn boundary_stream(
    fetch_container: FetchContainer,
) -> Result<BoxStream<'static, Result<(ChangesetId, MPath, impl GitDeltaManifestEntryOps)>>> {
    let objects = boundary_trees_and_blobs(fetch_container)
        .await?
        .into_iter()
        .map(|full_entry| {
            Ok((
                full_entry.cs_id.clone(),
                full_entry.path.clone(),
                full_entry.into_delta_manifest_entry(),
            ))
        });
    Ok(stream::iter(objects).boxed())
}

async fn packfile_stream_from_objects<'a>(
    fetch_container: FetchContainer,
    base_set: Arc<FxHashSet<ObjectId>>,
    object_stream: BoxStream<
        'a,
        Result<(
            ChangesetId,
            MPath,
            impl GitDeltaManifestEntryOps + Send + 'a,
        )>,
    >,
) -> BoxStream<'a, Result<PackfileItem>> {
    let FetchContainer {
        ctx,
        blobstore,
        delta_inclusion,
        filter,
        concurrency,
        packfile_item_inclusion,
        ..
    } = fetch_container;
    let delta_filter = filter.clone();
    object_stream
        .try_filter_map(move |(cs_id, path, entry)| {
            let base_set = base_set.clone();
            let filter = filter.clone();
            async move {
                let object_id = entry.full_object_oid();
                let (kind, size) = (entry.full_object_kind(), entry.full_object_size());
                if base_set.contains(&object_id) {
                    // This object is already present at the client, so do not include it in the packfile
                    Ok(None)
                } else if !filter_object(filter, &path, kind, size) {
                    // This object does not pass the filter specified by the client, so do not include it in the packfile
                    Ok(None)
                } else {
                    Ok(Some((cs_id, path, entry)))
                }
            }
        })
        // We use map + buffered instead of map_ok + try_buffered since weighted buffering for futures
        // currently exists only for Stream and not for TryStream
        .map(move |result| {
            match result {
                Err(err) => (0, Either::Left(future::err(err))),
                std::result::Result::Ok((changeset_id, path, entry)) => {
                    cloned!(ctx, blobstore);
                    let filter = delta_filter.clone();
                    let delta = delta_base(&entry, delta_inclusion, filter);
                    let weight = delta.as_ref().map_or(entry.full_object_size(), |delta| {
                        delta.instructions_compressed_size()
                    }) as usize;
                    let weight = std::cmp::max(weight / THRESHOLD_BYTES, 1);
                    let fetch_future = async move {
                        match delta {
                            Some(delta) => {
                                let instruction_bytes = delta
                                    .instruction_bytes(&ctx, &blobstore, changeset_id, path)
                                    .await?;

                                let packfile_item = PackfileItem::new_delta(
                                    entry.full_object_oid(),
                                    delta.base_object_oid(),
                                    delta.instructions_uncompressed_size(),
                                    instruction_bytes,
                                );
                                Ok(packfile_item)
                            }
                            None => {
                                // Use the full object instead
                                base_packfile_item(
                                    ctx.clone(),
                                    blobstore.clone(),
                                    ObjectIdentifierType::AllObjects(GitIdentifier::Rich(
                                        entry.full_object_rich_git_sha1()?,
                                    )),
                                    packfile_item_inclusion,
                                )
                                .await
                            }
                        }
                    };
                    (weight, Either::Right(fetch_future))
                }
            }
        })
        .buffered_weighted_bounded(concurrency.trees_and_blobs, concurrency.memory_bound)
        .boxed()
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn tree_and_blob_packfile_stream<'a>(
    fetch_container: FetchContainer,
    target_commits: BoxStream<'a, Result<ChangesetId>>,
    base_set: Arc<FxHashSet<ObjectId>>,
    tree_and_blob_shas: Vec<ObjectId>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let FetchContainer {
        ctx,
        blobstore,
        derived_data,
        concurrency,
        packfile_item_inclusion,
        ..
    } = fetch_container.clone();
    let (spare_blobstore, spare_ctx) = (blobstore.clone(), ctx.clone());
    let packfile_item_stream = target_commits
        .map_ok(move |changeset_id| {
            let blobstore = blobstore.clone();
            let derived_data = derived_data.clone();
            let ctx = ctx.clone();
            tree_and_blob_packfile_items(
                ctx,
                blobstore,
                derived_data,
                fetch_container.git_delta_manifest_version,
                changeset_id,
            )
        })
        .try_buffered(concurrency.trees_and_blobs * 2)
        .try_flatten()
        .boxed();

    let boundary_packfile_item_stream = packfile_stream_from_objects(
        fetch_container.clone(),
        base_set.clone(),
        boundary_stream(fetch_container.clone()).await?,
    )
    .await;

    let packfile_item_stream = packfile_stream_from_objects(
        fetch_container.clone(),
        base_set.clone(),
        packfile_item_stream,
    )
    .await;

    let requested_trees_and_blobs = stream::iter(tree_and_blob_shas.into_iter().map(Ok))
        .map_ok(move |oid| {
            let blobstore = spare_blobstore.clone();
            let ctx = spare_ctx.clone();
            async move {
                base_packfile_item(
                    ctx,
                    blobstore,
                    ObjectIdentifierType::AllObjects(GitIdentifier::Basic(
                        GitSha1::from_object_id(&oid)?,
                    )),
                    packfile_item_inclusion,
                )
                .await
            }
        })
        .try_buffered(concurrency.trees_and_blobs)
        .boxed();
    Ok(boundary_packfile_item_stream
        .chain(packfile_item_stream)
        .chain(requested_trees_and_blobs)
        .boxed())
}

/// Create a stream of packfile items containing commit objects that need to be included in the packfile/bundle.
/// Return the number of commit objects included in the stream along with the stream
async fn commit_packfile_stream<'a>(
    fetch_container: FetchContainer,
    repo: &'a impl Repo,
    target_commits: Vec<ChangesetId>,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    let mut commit_count = target_commits.len();
    let FetchContainer {
        blobstore,
        ctx,
        packfile_item_inclusion,
        concurrency,
        shallow_info,
        ..
    } = fetch_container;
    let shallow_commits = match shallow_info.as_ref() {
        Some(shallow_info) => shallow_info.boundary_commits.clone(),
        None => Vec::new(),
    };
    commit_count += shallow_commits.len();
    let commit_stream = to_commit_stream(shallow_commits)
        .chain(to_commit_stream(target_commits))
        .map_ok(move |changeset_id| {
            let blobstore = blobstore.clone();
            let ctx = ctx.clone();
            async move {
                let maybe_git_sha1 = repo
                .bonsai_git_mapping()
                .get_git_sha1_from_bonsai(&ctx, changeset_id)
                .await
                .with_context(|| {
                    format!(
                        "Error in fetching Git Sha1 for changeset {:?} through BonsaiGitMapping",
                        changeset_id
                    )
                })?;
                let git_sha1 = maybe_git_sha1.ok_or_else(|| {
                    anyhow::anyhow!("Git Sha1 not found for changeset {:?}", changeset_id)
                })?;
                let git_objectid = git_sha1.to_object_id()?;
                base_packfile_item(
                    ctx.clone(),
                    blobstore,
                    ObjectIdentifierType::NonBlobObjects(git_objectid), // Since we know its not a blob
                    packfile_item_inclusion,
                )
                .await
            }
        })
        .try_buffered(concurrency.commits)
        .boxed();
    Ok((commit_stream, commit_count))
}

/// Convert the provided tag entries into a stream of packfile items
fn tag_entries_to_stream<'a>(
    fetch_container: FetchContainer,
    tag_entries: Vec<BonsaiTagMappingEntry>,
) -> BoxStream<'a, Result<PackfileItem>> {
    let FetchContainer {
        ctx,
        blobstore,
        packfile_item_inclusion,
        concurrency,
        ..
    } = fetch_container;
    stream::iter(tag_entries.into_iter().map(Ok))
        .map_ok(move |entry| {
            let blobstore = blobstore.clone();
            let ctx = ctx.clone();
            async move {
                let git_objectid = entry.tag_hash.to_object_id()?;
                base_packfile_item(
                    ctx,
                    blobstore.clone(),
                    ObjectIdentifierType::NonBlobObjects(git_objectid), // Since we know its not a blob
                    packfile_item_inclusion,
                )
                .await
            }
        })
        .try_buffered(concurrency.tags)
        .boxed()
}

/// Create a stream of packfile items containing tag objects that need to be included in the packfile/bundle while also
/// returning the total number of tags included in the stream
async fn tag_packfile_stream<'a>(
    fetch_container: FetchContainer,
    repo: &'a impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    // Since we need the count of items, we would have to consume the stream either for counting or collecting the items.
    // This is fine, since unlike commits, blobs and trees there will only be thousands of tags in the worst case.
    let annotated_tags = stream::iter(bookmarks.keys())
        .filter_map(|bookmark| async move {
            // If the bookmark is actually a tag but there is no mapping in bonsai_tag_mapping table for it, then it
            // means that its a simple tag and won't be included in the packfile as an object. If a mapping exists, then
            // it will be included in the packfile as a raw Git object
            if bookmark.is_tag() {
                let tag_name = bookmark.name().to_string();
                repo.bonsai_tag_mapping()
                    .get_entry_by_tag_name(tag_name.clone())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in gettting bonsai_tag_mapping entry for tag name {}",
                            tag_name
                        )
                    })
                    .transpose()
            } else {
                None
            }
        })
        .try_collect::<Vec<_>>()
        .await?;
    let tags_count = annotated_tags.len();
    let tag_stream = tag_entries_to_stream(fetch_container, annotated_tags);
    Ok((tag_stream, tags_count))
}

/// Create a stream of packfile items containing annotated tag objects that exist in the repo
/// and point to a commit within the set of commits requested by the client
async fn tags_packfile_stream<'a>(
    fetch_container: FetchContainer,
    repo: &'a impl Repo,
    requested_commits: Vec<ChangesetId>,
    requested_tag_names: Arc<FxHashSet<String>>,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    let (ctx, filter) = (fetch_container.ctx.clone(), fetch_container.filter.clone());
    let include_tags = if let Some(filter) = filter.as_ref() {
        filter.include_tags()
    } else {
        true
    };
    let requested_commits: Arc<FxHashSet<ChangesetId>> =
        Arc::new(requested_commits.into_iter().collect());
    // Fetch all the tags that point to some commit in the given set of commits.
    // NOTE: Fun git trick. If the client says it doesn't want tags, then instead of excluding all tags (like regular systems)
    // we still send the tags that were explicitly part of the client's WANT request :)
    let required_tag_names = match include_tags {
        true => {
            repo.bookmarks_cache()
                .list(
                    &ctx,
                    &BookmarkPrefix::new(TAGS_PREFIX)?,
                    &BookmarkPagination::FromStart,
                    None, // Limit
                )
                .await
                .map(|entries| {
                    entries
                        .into_iter()
                        .filter_map(|(bookmark, (cs_id, _))| {
                            if requested_commits.contains(&cs_id) {
                                Some(bookmark.name().to_string())
                            } else {
                                None
                            }
                        })
                        .collect::<FxHashSet<_>>()
                })
                .context("Error in getting tags pointing to input set of commits")?
        }
        false => FxHashSet::default(),
    };
    // Fetch entries corresponding to annotated tags in the repo or with names
    // that match the requested tag names
    let tag_entries = repo
        .bonsai_tag_mapping()
        .get_all_entries()
        .await
        .context("Error in getting tags during fetch")?
        .into_iter()
        .filter(|entry| {
            required_tag_names.contains(&entry.tag_name)
                || requested_tag_names.contains(&entry.tag_name)
        })
        .collect::<Vec<_>>();
    let tags_count = tag_entries.len();
    let tag_stream = tag_entries_to_stream(fetch_container, tag_entries);
    Ok((tag_stream, tags_count))
}

/// Based on the input request parameters, generate a stream of `PackfileItem`s that
/// can be written into a pack file
pub async fn generate_pack_item_stream<'a>(
    ctx: CoreContext,
    repo: &'a impl Repo,
    request: PackItemStreamRequest,
) -> Result<PackItemStreamResponse<'a>> {
    // We need to include the bookmarks (i.e. branches, tags) in the pack based on the request parameters
    let bookmarks = bookmarks(&ctx, repo, &request.requested_refs)
        .await
        .with_context(|| {
            format!(
                "Error in fetching bookmarks for repo {}",
                repo.repo_identity().name()
            )
        })?;
    let ctx = Arc::new(ctx);
    let fetch_container = FetchContainer::new(
        ctx.clone(),
        repo,
        request.delta_inclusion,
        Arc::new(None),
        request.concurrency,
        request.packfile_item_inclusion,
        Arc::new(None),
    )?;
    // Get all the commits that are reachable from the bookmarks
    let mut target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            &ctx,
            bookmarks.values().copied().collect(),
            request.have_heads.clone(),
        )
        .await
        .context("Error in getting ancestors difference while generating packitem stream")?
        .try_collect::<Vec<_>>()
        .await?;
    // Reverse the list of commits so that we can prevent delta cycles from appearing in the packfile
    target_commits.reverse();
    // STEP 1: Get the count of distinct blob and tree objects to be included in the packfile/bundle.
    let (trees_and_blobs_count, base_set) = trees_and_blobs_count(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
    )
    .await
    .context("Error while calculating object count")?;

    // STEP 2: Create a mapping of all known bookmarks (i.e. branches, tags) and the commit that they point to. The commit should be represented
    // as a Git hash instead of a Bonsai hash since it will be part of the packfile/bundle
    let mut refs_to_include = refs_to_include(&ctx, repo, &bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the pack")?;

    // STEP 2.5: Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(repo, request.requested_symrefs, &mut refs_to_include)
        .await
        .context("Error while adding symrefs to included set of refs")?;

    // STEP 3: Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let tree_and_blob_stream = tree_and_blob_packfile_stream(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
        Arc::new(base_set),
        vec![],
    )
    .await
    .context("Error while generating blob and tree packfile item stream")?;

    // STEP 4: Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let (commit_stream, commits_count) =
        commit_packfile_stream(fetch_container.clone(), repo, target_commits.clone())
            .await
            .context("Error while generating commit packfile item stream")?;

    // STEP 5: Get the stream of tag packfile items to include in the pack/bundle. Note that we have not yet included the tag count in the
    // total object count so we will need the stream + count of elements in the stream
    let (tag_stream, tags_count) = tag_packfile_stream(fetch_container.clone(), repo, &bookmarks)
        .await
        .context("Error while generating tag packfile item stream")?;
    // Compute the overall object count by summing the trees, blobs, tags and commits count
    let object_count = commits_count + trees_and_blobs_count + tags_count;

    // STEP 6: Combine all streams together and return the response. The ordering of the streams in this case is irrelevant since the commit
    // and tag stream include full objects and the tree_and_blob_stream has deltas in the correct order
    let packfile_stream = tag_stream
        .chain(commit_stream)
        .chain(tree_and_blob_stream)
        .boxed();
    let response = PackItemStreamResponse::new(
        packfile_stream,
        object_count,
        refs_to_include.into_iter().collect(),
    );
    Ok(response)
}

/// Based on the input request parameters, generate the response to the
/// ls-refs request command
pub async fn ls_refs_response(
    ctx: &CoreContext,
    repo: &impl Repo,
    request: LsRefsRequest,
) -> Result<LsRefsResponse> {
    // We need to include the bookmarks (i.e. branches, tags) based on the request parameters
    let bookmarks = bookmarks(ctx, repo, &request.requested_refs)
        .await
        .with_context(|| {
            format!(
                "Error in fetching bookmarks for repo {}",
                repo.repo_identity().name()
            )
        })?;
    // Convert the above bookmarks into refs that can be sent in the response
    let mut refs_to_include = refs_to_include(ctx, repo, &bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the response")?;

    // Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(repo, request.requested_symrefs, &mut refs_to_include)
        .await
        .context("Error while adding symrefs to included set of refs")?;

    Ok(LsRefsResponse::new(refs_to_include.into_iter().collect()))
}

/// Based on the input request parameters, generate the response to the
/// fetch request command
pub async fn fetch_response<'a>(
    ctx: CoreContext,
    repo: &'a impl Repo,
    mut request: FetchRequest,
) -> Result<FetchResponse<'a>> {
    let delta_inclusion = DeltaInclusion::standard();
    let filter = Arc::new(request.filter.clone());
    let packfile_item_inclusion = PackfileItemInclusion::FetchAndStore;
    let ctx = Arc::new(ctx);
    let shallow_info = Arc::new(request.shallow_info.take());
    let fetch_container = FetchContainer::new(
        ctx.clone(),
        repo,
        delta_inclusion,
        filter.clone(),
        request.concurrency,
        packfile_item_inclusion,
        shallow_info.clone(),
    )?;
    // Convert the base commits and head commits, which are represented as Git hashes, into Bonsai hashes
    // If the input contains tag object Ids, fetch the corresponding tag names
    let translated_sha_bases = git_shas_to_bonsais(&ctx, repo, request.bases.iter())
        .await
        .context("Error converting base Git commits to Bonsai duing fetch")?;
    let translated_sha_heads = git_shas_to_bonsais(&ctx, repo, request.heads.iter())
        .await
        .context("Error converting head Git commits to Bonsai during fetch")?;
    // Get the stream of commits between the bases and heads
    // NOTE: Another Git magic. The filter spec includes an option that the client can use to exclude commit-type objects. But, even if the client
    // uses that filter, we just ignore it and send all the commits anyway :)
    let mut target_commits = commits(
        &ctx,
        repo,
        translated_sha_heads.bonsais.clone(),
        translated_sha_bases.bonsais.clone(),
        &shallow_info,
    )
    .await?;
    // Reverse the list of commits so that we can prevent delta cycles from appearing in the packfile
    target_commits.reverse();
    // Get the count of unique blob and tree objects to be included in the packfile
    let (trees_and_blobs_count, base_set) = trees_and_blobs_count(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
    )
    .await
    .context("Error while calculating object count during fetch")?;
    // Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let explicitly_requested_trees_and_blobs_count =
        translated_sha_heads.non_tag_non_commit_oids.len();
    let tree_and_blob_stream = tree_and_blob_packfile_stream(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
        Arc::new(base_set),
        translated_sha_heads.non_tag_non_commit_oids,
    )
    .await
    .context("Error while generating blob and tree packfile item stream during fetch")?;
    // Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let (commit_stream, commits_count) =
        commit_packfile_stream(fetch_container.clone(), repo, target_commits.clone())
            .await
            .context("Error while generating commit packfile item stream during fetch")?;
    // Get the stream of all annotated tag items in the repo
    // NOTE: Ideally, we should filter it based on the requested refs but its much faster to just send all the tags.
    // Git ignores the unnecessary objects and the extra size overhead in the pack is just a few KBs
    let (tag_stream, tags_count) = tags_packfile_stream(
        fetch_container,
        repo,
        target_commits,
        translated_sha_heads.tag_names.clone(),
    )
    .await
    .context("Error while generating tag packfile item stream during fetch")?;
    // Compute the overall object count by summing the trees, blobs, tags and commits count
    println!(
        "trees_and_blobs_count: {}, commits_count: {}, tags_count: {}",
        trees_and_blobs_count, commits_count, tags_count
    );
    let object_count = commits_count
        + trees_and_blobs_count
        + tags_count
        + explicitly_requested_trees_and_blobs_count;
    // Combine all streams together and return the response. The ordering of the streams in this case is irrelevant since the commit
    // and tag stream include full objects and the tree_and_blob_stream has deltas in the correct order
    let packfile_stream = tag_stream
        .chain(commit_stream)
        .chain(tree_and_blob_stream)
        .boxed();
    Ok(FetchResponse::new(packfile_stream, object_count))
}

/// Based on the input request parameters, generate the information for shallow info section
pub async fn shallow_info(
    ctx: CoreContext,
    repo: &impl Repo,
    request: ShallowInfoRequest,
) -> Result<ShallowInfoResponse> {
    let ctx = Arc::new(ctx);
    // Convert the requested head object ids to bonsais so that we can use Mononoke commit graph
    let translated_sha_heads = git_shas_to_bonsais(&ctx, repo, request.heads.iter())
        .await
        .context("Error converting head Git commits to Bonsai during shallow-info")?;
    // Convert the requested shallow object ids to bonsais so that we can use Mononoke commit graph
    let translated_shallow_commits = git_shas_to_bonsais(&ctx, repo, request.shallow.iter())
        .await
        .context("Error converting shallow Git commits to Bonsai during shallow-info")?;
    let shallow_bonsais = translated_shallow_commits.bonsais.clone();
    let ancestors_within_distance = match &request.variant {
        ShallowVariant::FromServerWithDepth(depth) => repo
            .commit_graph()
            .ancestors_within_distance(&ctx, translated_sha_heads.bonsais, (*depth - 1) as u64)
            .await
            .context("Error in getting ancestors within distance from heads commits during shallow-info")?,
        ShallowVariant::FromClientWithDepth(depth) => repo
            .commit_graph()
            .ancestors_within_distance(&ctx, translated_shallow_commits.bonsais, (*depth - 1) as u64)
            .await
            .context("Error in getting ancestors within distance from shallow commits during shallow-info")?,
        ShallowVariant::None => AncestorsWithinDistance::default(),
        variant => anyhow::bail!("Shallow variant {:?} is not supported yet", variant),
    };
    Ok(ShallowInfoResponse::new(
        ancestors_within_distance.ancestors,
        ancestors_within_distance.boundaries,
        shallow_bonsais,
    ))
}
