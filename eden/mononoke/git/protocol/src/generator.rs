/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
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
use bytes::BytesMut;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::future;
use futures::future::Either;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt as _;
use futures::TryStreamExt;
use git_symbolic_refs::GitSymbolicRefsRef;
use git_types::fetch_delta_instructions;
use git_types::fetch_git_object_bytes;
use git_types::fetch_non_blob_git_object_bytes;
use git_types::fetch_packfile_base_item;
use git_types::fetch_packfile_base_item_if_exists;
use git_types::upload_packfile_base_item;
use git_types::DeltaInstructionChunkIdPrefix;
use git_types::GitDeltaManifestEntry;
use git_types::HeaderState;
use git_types::ObjectDelta;
use git_types::RootGitDeltaManifestId;
use gix_hash::ObjectId;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use packfile::types::PackfileItem;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::types::DeltaInclusion;
use crate::types::FetchRequest;
use crate::types::FetchResponse;
use crate::types::LsRefsRequest;
use crate::types::LsRefsResponse;
use crate::types::PackItemStreamRequest;
use crate::types::PackItemStreamResponse;
use crate::types::PackfileItemInclusion;
use crate::types::RefTarget;
use crate::types::RequestedRefs;
use crate::types::RequestedSymrefs;
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
    + Send
    + Sync;

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
            anyhow::Ok((ref_name, oid.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    anyhow::Ok(wanted_refs_with_oid.into_iter())
}

/// Get the count of distinct blob and tree items to be included in the packfile
async fn trees_and_blobs_count(
    ctx: &CoreContext,
    repo: &impl Repo,
    target_commits: BoxStream<'_, Result<ChangesetId>>,
    concurrency: usize,
) -> Result<usize> {
    // Sum up the entries in the delta manifest for each commit included in packfile
    target_commits
        .map_ok(|changeset_id| {
            async move {
                let blobstore = repo.repo_blobstore_arc();
                let root_mf_id = repo
                    .repo_derived_data()
                    .derive::<RootGitDeltaManifestId>(ctx, changeset_id)
                    .await
                    .with_context(|| {
                        format!(
                            "Error in deriving RootGitDeltaManifestId for commit {:?}",
                            changeset_id
                        )
                    })?;
                let delta_manifest = root_mf_id
                    .manifest_id()
                    .load(ctx, &blobstore)
                    .await
                    .with_context(|| {
                        format!(
                            "Error in loading Git Delta Manifest from root id {:?}",
                            root_mf_id
                        )
                    })?;
                // Get the FxHashSet of the tree and blob object Ids that will be included
                // in the packfile
                let objects = delta_manifest
                    .into_subentries(ctx, &blobstore)
                    .map_ok(|(_, entry)| entry.full.oid)
                    .try_collect::<FxHashSet<_>>()
                    .await
                    .with_context(|| {
                        format!(
                            "Error while listing entries from GitDeltaManifest {:?}",
                            root_mf_id
                        )
                    })?;
                anyhow::Ok(objects)
            }
        })
        .try_buffer_unordered(concurrency)
        .try_concat()
        .await
        .map(|objects| objects.len())
}

fn delta_below_threshold(
    delta: &ObjectDelta,
    entry: &GitDeltaManifestEntry,
    inclusion_threshold: f32,
) -> bool {
    (delta.instructions_compressed_size as f64)
        < (entry.full.size as f64) * inclusion_threshold as f64
}

fn delta_base(
    entry: &mut GitDeltaManifestEntry,
    delta_inclusion: DeltaInclusion,
) -> Option<ObjectDelta> {
    match delta_inclusion {
        DeltaInclusion::Include {
            inclusion_threshold,
            ..
        } => {
            entry.deltas.sort_by(|a, b| {
                a.instructions_compressed_size
                    .cmp(&b.instructions_compressed_size)
            });
            entry
                .deltas
                .first()
                .filter(|delta| delta_below_threshold(delta, entry, inclusion_threshold))
                .cloned()
        }
        // Can't use the delta variant if the request prevents us from using it
        DeltaInclusion::Exclude => None,
    }
}

fn to_commit_stream(commits: Vec<ChangesetId>) -> BoxStream<'static, Result<ChangesetId>> {
    stream::iter(commits.into_iter().map(Ok)).boxed()
}

/// Fetch all the bonsai commits pointed to by the annotated tags corresponding
/// to the input object ids
async fn tagged_commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    git_shas: Vec<GitSha1>,
) -> Result<Vec<ChangesetId>> {
    if git_shas.is_empty() {
        return Ok(vec![]);
    }
    // Fetch the names of the tags corresponding to the tag object represented by the input object ids
    let tag_names = repo
        .bonsai_tag_mapping()
        .get_entries_by_tag_hashes(git_shas)
        .await
        .context("Error while fetching tag entries from tag hashes")?
        .into_iter()
        .map(|entry| entry.tag_name)
        .collect::<FxHashSet<String>>();
    let tag_names = Arc::new(tag_names);
    // Fetch the commits pointed to by those tags
    repo.bookmarks_cache()
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
        })
}

/// Fetch the corresponding bonsai commits for the input Git object ids. If the object id doesn't
/// correspond to a bonsai commit, try to resolve it to a tag and then fetch the bonsai commit
async fn git_shas_to_bonsais(
    ctx: &CoreContext,
    repo: &impl Repo,
    oids: impl Iterator<Item = impl AsRef<gix_hash::oid>>,
) -> Result<Vec<ChangesetId>> {
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
    let mut commits_from_tags = tagged_commits(ctx, repo, tag_shas)
        .await
        .context("Error while resolving annotated tags to their commits")?;
    commits_from_tags.extend(entries.into_iter().map(|entry| entry.bcs_id));
    anyhow::Ok(commits_from_tags)
}

/// Fetch the Bonsai Git Mappings for the given bonsais
async fn bonsai_git_mappings_by_bonsai(
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
        .map(|entry| anyhow::Ok((entry.bcs_id, entry.git_sha1.to_object_id()?)))
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
        .map(|entry| anyhow::Ok((entry.tag_name, entry.tag_hash.to_object_id()?)))
        .collect::<Result<FxHashMap<_, _>>>()?;

    bookmarks.iter().map(|(bookmark, cs_id)| {
        if bookmark.is_tag() {
            match tag_inclusion {
                TagInclusion::AsIs => {
                    if let Some(git_objectid) = bonsai_tag_map.get(&bookmark.to_string()) {
                        let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                        return anyhow::Ok((ref_name, RefTarget::Plain(git_objectid.clone())));
                    }
                }
                TagInclusion::Peeled => {
                    let git_objectid = bonsai_git_map.get(cs_id).ok_or_else(|| {
                        anyhow::anyhow!("No Git ObjectId found for changeset {:?} during refs-to-include", cs_id)
                    })?;
                    let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                    return anyhow::Ok((ref_name, RefTarget::Plain(git_objectid.clone())));
                }
                TagInclusion::WithTarget => {
                    if let Some(tag_objectid) = bonsai_tag_map.get(&bookmark.to_string()) {
                        let commit_objectid = bonsai_git_map.get(cs_id).ok_or_else(|| {
                            anyhow::anyhow!("No Git ObjectId found for changeset {:?} during refs-to-include", cs_id)
                        })?;
                        let ref_name = format!("{}{}", REF_PREFIX, bookmark);
                        let metadata = format!("peeled:{}", commit_objectid.to_hex());
                        return anyhow::Ok((
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
        anyhow::Ok((ref_name, RefTarget::Plain(git_objectid.clone())))
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
    /// A RichGitSha1 hash has information about the type and size of the object
    /// and hence can be used as an identifier for all types of Git objects
    AllObjects(RichGitSha1),
    /// The ObjectId cannot provide type and size information and hence should be
    /// used only when the object is NOT a blob
    NonBlobObjects(ObjectId),
}

impl ObjectIdentifierType {
    pub fn to_object_id(&self) -> Result<ObjectId> {
        match self {
            Self::AllObjects(sha) => Ok(sha.to_object_id()?),
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
        ObjectIdentifierType::AllObjects(sha) => {
            // The object identifier has been passed along with size and type information. This means
            // that it can be any type of Git object. We store Git blobs as file content and all other
            // Git objects as raw git content. The fetch_git_object_bytes function fetches from the appropriate
            // source depending on the type of the object.
            fetch_git_object_bytes(ctx, blobstore.clone(), &sha, HeaderState::Included).await?
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
            anyhow::Ok(packfile_item)
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
            anyhow::Ok(PackfileItem::new_encoded_base(
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
                Some(packfile_base_item) => anyhow::Ok(PackfileItem::new_encoded_base(
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
                    anyhow::Ok(PackfileItem::new_encoded_base(
                        packfile_base_item.try_into()?,
                    ))
                }
            }
        }
    }
}

/// Fetch the stream of blob and tree objects as delta manifest entries for the given changeset
async fn blob_and_tree_packfile_items(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    derived_data: ArcRepoDerivedData,
    changeset_id: ChangesetId,
) -> Result<BoxStream<'static, Result<(ChangesetId, MPath, GitDeltaManifestEntry)>>> {
    let root_mf_id = derived_data
        .derive::<RootGitDeltaManifestId>(&ctx, changeset_id)
        .await
        .with_context(|| {
            format!(
                "Error in deriving RootGitDeltaManifestId for commit {:?}",
                changeset_id
            )
        })?;
    let delta_manifest = root_mf_id
        .manifest_id()
        .load(&ctx, &blobstore)
        .await
        .with_context(|| {
            format!(
                "Error in loading Git Delta Manifest from root id {:?}",
                root_mf_id
            )
        })?;
    // Most delta manifests would contain tens of entries. These entries are just metadata and
    // not the actual object so its safe to load them all into memory instead of chaining streams
    // which significantly slows down the entire process.
    let entries = delta_manifest
        .into_subentries(&ctx, &blobstore)
        .map_ok(|(path, entry)| (changeset_id, path, entry))
        .try_collect::<Vec<_>>()
        .await?;
    anyhow::Ok(stream::iter(entries.into_iter().map(anyhow::Ok)).boxed())
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn blob_and_tree_packfile_stream<'a>(
    ctx: Arc<CoreContext>,
    blobstore: ArcRepoBlobstore,
    derived_data: ArcRepoDerivedData,
    target_commits: BoxStream<'a, Result<ChangesetId>>,
    delta_inclusion: DeltaInclusion,
    packfile_item_inclusion: PackfileItemInclusion,
    concurrency: usize,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let second_blobstore = blobstore.clone();
    let second_ctx = ctx.clone();
    let packfile_item_stream = target_commits
        .map_ok(move |changeset_id| {
            let blobstore = blobstore.clone();
            let derived_data = derived_data.clone();
            let ctx = ctx.clone();
            blob_and_tree_packfile_items(ctx, blobstore, derived_data, changeset_id)
        })
        .try_buffered(concurrency * 2)
        .try_flatten()
        // We use map + buffered instead of map_ok + try_buffered since weighted buffering for futures
        // currently exists only for Stream and not for TryStream
        .map(move |result| {
            match result {
                Err(err) => (0, Either::Left(future::err(err))),
                Ok((changeset_id, path, mut entry)) => {
                    let ctx = second_ctx.clone();
                    let blobstore = second_blobstore.clone();
                    let delta = delta_base(&mut entry, delta_inclusion);
                    let weight = delta
                        .as_ref()
                        .map_or(entry.full.size, |delta| delta.instructions_compressed_size)
                        as usize;
                    let weight = std::cmp::max(weight / THRESHOLD_BYTES, 1);
                    let fetch_future = async move {
                        match delta {
                            Some(delta) => {
                                let chunk_id_prefix = DeltaInstructionChunkIdPrefix::new(
                                    changeset_id,
                                    path.clone(),
                                    delta.origin,
                                    path,
                                );
                                let instruction_bytes = fetch_delta_instructions(
                                    &ctx,
                                    &blobstore,
                                    &chunk_id_prefix,
                                    delta.instructions_chunk_count,
                                )
                                .try_fold(
                                    BytesMut::with_capacity(
                                        delta.instructions_compressed_size as usize,
                                    ),
                                    |mut acc, bytes| async move {
                                        acc.extend_from_slice(bytes.as_ref());
                                        anyhow::Ok(acc)
                                    },
                                )
                                .await
                                .context(
                                    "Error in fetching delta instruction bytes from byte stream",
                                )?
                                .freeze();

                                let packfile_item = PackfileItem::new_delta(
                                    entry.full.oid,
                                    delta.base.oid,
                                    delta.instructions_uncompressed_size,
                                    instruction_bytes,
                                );
                                anyhow::Ok(packfile_item)
                            }
                            None => {
                                // Use the full object instead
                                base_packfile_item(
                                    ctx.clone(),
                                    blobstore.clone(),
                                    ObjectIdentifierType::AllObjects(
                                        entry.full.as_rich_git_sha1()?,
                                    ),
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
        .buffered_weighted(concurrency)
        .boxed();
    Ok(packfile_item_stream)
}

/// Create a stream of packfile items containing commit objects that need to be included in the packfile/bundle
async fn commit_packfile_stream<'a>(
    ctx: Arc<CoreContext>,
    repo: &'a impl Repo,
    target_commits: BoxStream<'a, Result<ChangesetId>>,
    packfile_item_inclusion: PackfileItemInclusion,
    concurrency: usize,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let blobstore = repo.repo_blobstore_arc();
    let commit_stream = target_commits
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
        .try_buffered(concurrency)
        .boxed();
    anyhow::Ok(commit_stream)
}

/// Convert the provided tag entries into a stream of packfile items
fn tag_entries_to_stream<'a>(
    ctx: Arc<CoreContext>,
    repo: &'a impl Repo,
    tag_entries: Vec<BonsaiTagMappingEntry>,
    packfile_item_inclusion: PackfileItemInclusion,
    concurrency: usize,
) -> BoxStream<'a, Result<PackfileItem>> {
    let blobstore = repo.repo_blobstore_arc();
    stream::iter(tag_entries.into_iter().map(anyhow::Ok))
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
        .try_buffered(concurrency)
        .boxed()
}

/// Create a stream of packfile items containing tag objects that need to be included in the packfile/bundle while also
/// returning the total number of tags included in the stream
async fn tag_packfile_stream<'a>(
    ctx: Arc<CoreContext>,
    repo: &'a impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    request: &PackItemStreamRequest,
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
    let tag_stream = tag_entries_to_stream(
        ctx,
        repo,
        annotated_tags,
        request.packfile_item_inclusion,
        request.concurrency.tags,
    );
    anyhow::Ok((tag_stream, tags_count))
}

/// Create a stream of packfile items containing annotated tag objects that exist in the repo
/// and point to a commit within the set of commits requested by the client
async fn tags_packfile_stream<'a>(
    ctx: Arc<CoreContext>,
    repo: &'a impl Repo,
    requested_commits: Vec<ChangesetId>,
    packfile_item_inclusion: PackfileItemInclusion,
    concurrency: usize,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    let requested_commits: Arc<FxHashSet<ChangesetId>> =
        Arc::new(requested_commits.into_iter().collect());
    // Fetch all the tags that point to some commit in the given set of commits
    let required_tag_names = repo
        .bookmarks_cache()
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
        .context("Error in getting tags pointing to input set of commits")?;
    // Fetch entries corresponding to annotated tags in the repo
    let tag_entries = repo
        .bonsai_tag_mapping()
        .get_all_entries()
        .await
        .context("Error in getting tags during fetch")?
        .into_iter()
        .filter(|entry| required_tag_names.contains(&entry.tag_name))
        .collect::<Vec<_>>();
    let tags_count = tag_entries.len();
    let tag_stream =
        tag_entries_to_stream(ctx, repo, tag_entries, packfile_item_inclusion, concurrency);
    anyhow::Ok((tag_stream, tags_count))
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
    let commits_count = target_commits.len();
    // STEP 1: Get the count of distinct blob and tree objects to be included in the packfile/bundle.
    let trees_and_blobs_count = trees_and_blobs_count(
        &ctx,
        repo,
        to_commit_stream(target_commits.clone()),
        request.concurrency.trees_and_blobs,
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
    let blob_and_tree_stream = blob_and_tree_packfile_stream(
        ctx.clone(),
        repo.repo_blobstore_arc(),
        repo.repo_derived_data_arc(),
        to_commit_stream(target_commits.clone()),
        request.delta_inclusion,
        request.packfile_item_inclusion,
        request.concurrency.trees_and_blobs,
    )
    .await
    .context("Error while generating blob and tree packfile item stream")?;

    // STEP 4: Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let commit_stream = commit_packfile_stream(
        ctx.clone(),
        repo,
        to_commit_stream(target_commits.clone()),
        request.packfile_item_inclusion,
        request.concurrency.commits,
    )
    .await
    .context("Error while generating commit packfile item stream")?;

    // STEP 5: Get the stream of tag packfile items to include in the pack/bundle. Note that we have not yet included the tag count in the
    // total object count so we will need the stream + count of elements in the stream
    let (tag_stream, tags_count) = tag_packfile_stream(ctx.clone(), repo, &bookmarks, &request)
        .await
        .context("Error while generating tag packfile item stream")?;
    // Compute the overall object count by summing the trees, blobs, tags and commits count
    let object_count = commits_count + trees_and_blobs_count + tags_count;

    // STEP 6: Combine all streams together and return the response. The ordering of the streams in this case is irrelevant since the commit
    // and tag stream include full objects and the blob_and_tree_stream has deltas in the correct order
    let packfile_stream = tag_stream
        .chain(commit_stream)
        .chain(blob_and_tree_stream)
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
    request: FetchRequest,
) -> Result<FetchResponse<'a>> {
    let delta_inclusion = DeltaInclusion::standard();
    let packfile_item_inclusion = PackfileItemInclusion::FetchAndStore;
    let ctx = Arc::new(ctx);
    // Convert the base commits and head commits, which are represented as Git hashes, into Bonsai hashes
    let bases = git_shas_to_bonsais(&ctx, repo, request.bases.iter())
        .await
        .context("Error converting base Git commits to Bonsai duing fetch")?;
    let heads = git_shas_to_bonsais(&ctx, repo, request.heads.iter())
        .await
        .context("Error converting head Git commits to Bonsai during fetch")?;
    // Get the stream of commits between the bases and heads
    let mut target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(&ctx, heads, bases)
        .await
        .context("Error in getting stream of commits between heads and bases during fetch")?
        .try_collect::<Vec<_>>()
        .await?;
    let commits_count = target_commits.len();
    // Reverse the list of commits so that we can prevent delta cycles from appearing in the packfile
    target_commits.reverse();
    // Get the count of unique blob and tree objects to be included in the packfile
    let trees_and_blobs_count = trees_and_blobs_count(
        &ctx,
        repo,
        to_commit_stream(target_commits.clone()),
        request.concurrency.trees_and_blobs,
    )
    .await
    .context("Error while calculating object count during fetch")?;
    // Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let blob_and_tree_stream = blob_and_tree_packfile_stream(
        ctx.clone(),
        repo.repo_blobstore_arc(),
        repo.repo_derived_data_arc(),
        to_commit_stream(target_commits.clone()),
        delta_inclusion,
        packfile_item_inclusion,
        request.concurrency.trees_and_blobs,
    )
    .await
    .context("Error while generating blob and tree packfile item stream during fetch")?;
    // Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let commit_stream = commit_packfile_stream(
        ctx.clone(),
        repo,
        to_commit_stream(target_commits.clone()),
        packfile_item_inclusion,
        request.concurrency.trees_and_blobs,
    )
    .await
    .context("Error while generating commit packfile item stream during fetch")?;
    // Get the stream of all annotated tag items in the repo
    // NOTE: Ideally, we should filter it based on the requested refs but its much faster to just send all the tags.
    // Git ignores the unnecessary objects and the extra size overhead in the pack is just a few KBs
    let (tag_stream, tags_count) = tags_packfile_stream(
        ctx.clone(),
        repo,
        target_commits,
        packfile_item_inclusion,
        request.concurrency.tags,
    )
    .await
    .context("Error while generating tag packfile item stream during fetch")?;
    // Compute the overall object count by summing the trees, blobs, tags and commits count
    let object_count = commits_count + trees_and_blobs_count + tags_count;
    // Combine all streams together and return the response. The ordering of the streams in this case is irrelevant since the commit
    // and tag stream include full objects and the blob_and_tree_stream has deltas in the correct order
    let packfile_stream = tag_stream
        .chain(commit_stream)
        .chain(blob_and_tree_stream)
        .boxed();
    Ok(FetchResponse::new(packfile_stream, object_count))
}
