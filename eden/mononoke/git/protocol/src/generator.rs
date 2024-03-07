/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_stream::try_stream;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::Bytes;
use bytes::BytesMut;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
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
use mononoke_types::hash::RichGitSha1;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use packfile::types::PackfileItem;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::types::DeltaInclusion;
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

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + BookmarksRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + RepoDerivedDataRef
    + GitSymbolicRefsRef
    + CommitGraphRef
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
        .bookmarks()
        .list(
            ctx.clone(),
            Freshness::MostRecent,
            &BookmarkPrefix::empty(),
            BookmarkCategory::ALL,
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            u64::MAX,
        )
        .try_filter_map(|(bookmark, cs_id)| {
            let refs = requested_refs.clone();
            let name = bookmark.name().to_string();
            async move {
                let result = match refs {
                    RequestedRefs::Included(refs) if refs.contains(&name) => {
                        Some((bookmark.into_key(), cs_id))
                    }
                    RequestedRefs::IncludedWithPrefix(ref_prefixes) => {
                        let ref_name = format!("refs/{}", name);
                        if ref_prefixes
                            .iter()
                            .any(|ref_prefix| ref_name.starts_with(ref_prefix))
                        {
                            Some((bookmark.into_key(), cs_id))
                        } else {
                            None
                        }
                    }
                    RequestedRefs::Excluded(refs) if !refs.contains(&name) => {
                        Some((bookmark.into_key(), cs_id))
                    }
                    RequestedRefs::IncludedWithValue(refs) => refs
                        .get(&name)
                        .map(|cs_id| (bookmark.into_key(), cs_id.clone())),
                    _ => None,
                };
                anyhow::Ok(result)
            }
        })
        .try_collect::<FxHashMap<_, _>>()
        .await?;
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

/// Get the count of distinct blob and tree items to be included in the packfile
async fn object_count(
    ctx: &CoreContext,
    repo: &impl Repo,
    target_commits: BoxStream<'_, Result<ChangesetId>>,
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
        .try_buffer_unordered(1000)
        .try_concat()
        .await
        .map(|objects| objects.len())
}

/// Get the tag entry for the given tag key
async fn tag_entry(repo: &impl Repo, tag: &BookmarkKey) -> Result<Option<BonsaiTagMappingEntry>> {
    let tag_name = tag.name().to_string();
    repo.bonsai_tag_mapping()
        .get_entry_by_tag_name(tag_name.clone())
        .await
        .with_context(|| {
            format!(
                "Error in gettting bonsai_tag_mapping entry for tag name {}",
                tag_name
            )
        })
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

/// Get the Git counterpart for a bonsai commit, if it exists.
async fn bonsai_to_git_sha(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
) -> Result<ObjectId> {
    let maybe_git_sha1 = repo
        .bonsai_git_mapping()
        .get_git_sha1_from_bonsai(ctx, cs_id)
        .await
        .with_context(|| {
            format!(
                "Error in fetching Git Sha1 for changeset {:?} through BonsaiGitMapping",
                cs_id
            )
        })?;
    let git_sha1 = maybe_git_sha1
        .ok_or_else(|| anyhow::anyhow!("Git Sha1 not found for changeset {:?}", cs_id))?;
    ObjectId::from_hex(git_sha1.to_hex().as_bytes()).with_context(|| {
        format!(
            "Error in converting GitSha1 {:?} to GitObjectId",
            git_sha1.to_hex()
        )
    })
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
    stream::iter(bookmarks.iter())
        .map(|(bookmark, cs_id)| async move {
            if bookmark.is_tag() {
                match tag_inclusion {
                    TagInclusion::AsIs => {
                        if let Some(entry) = tag_entry(repo, bookmark).await? {
                            let git_objectid = entry.tag_hash.to_object_id()?;
                            let ref_name = format!("refs/{}", bookmark);
                            return anyhow::Ok((ref_name, RefTarget::Plain(git_objectid)));
                        }
                    }
                    TagInclusion::Peeled => {
                        let git_objectid = bonsai_to_git_sha(ctx, repo, *cs_id).await?;
                        let ref_name = format!("refs/{}", bookmark);
                        return anyhow::Ok((ref_name, RefTarget::Plain(git_objectid)));
                    }
                    TagInclusion::WithTarget => {
                        if let Some(entry) = tag_entry(repo, bookmark).await? {
                            let tag_objectid = entry.tag_hash.to_object_id()?;
                            let commit_objectid = bonsai_to_git_sha(ctx, repo, *cs_id).await?;
                            let ref_name = format!("refs/{}", bookmark);
                            let metadata = format!("peeled:{}", commit_objectid.to_hex());
                            return anyhow::Ok((
                                ref_name,
                                RefTarget::WithMetadata(tag_objectid, metadata),
                            ));
                        }
                    }
                }
            };
            // If the bookmark is a branch or if its just a simple (non-annotated) tag, we generate the
            // ref to target mapping based on the changeset id
            let git_objectid = bonsai_to_git_sha(ctx, repo, *cs_id).await?;
            let ref_name = format!("refs/{}", bookmark);
            anyhow::Ok((ref_name, RefTarget::Plain(git_objectid)))
        })
        .boxed()
        .buffer_unordered(1000)
        .try_collect::<FxHashMap<_, _>>()
        .await
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
    repo: &impl Repo,
    id: ObjectIdentifierType,
) -> Result<Bytes> {
    let blobstore = repo.repo_blobstore_arc();
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
    ctx: &CoreContext,
    repo: &impl Repo,
    id: ObjectIdentifierType,
    packfile_item_inclusion: PackfileItemInclusion,
) -> Result<PackfileItem> {
    let blobstore = repo.repo_blobstore_arc();
    let git_objectid = id.to_object_id()?;
    match packfile_item_inclusion {
        // Generate the packfile item based on the raw commit object
        PackfileItemInclusion::Generate => {
            let object_bytes = object_bytes(ctx, repo, id).await.with_context(|| {
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
                fetch_packfile_base_item(ctx, &blobstore, git_objectid.as_ref())
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
                ctx,
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
                    let object_bytes = object_bytes(ctx, repo, id).await.with_context(|| {
                        format!(
                            "Error in fetching raw git object bytes for object {:?} while fetching-and-storing packfile item",
                            &git_objectid
                        )
                    })?;
                    let packfile_base_item = upload_packfile_base_item(
                        ctx,
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

/// Generate a PackfileEntry for the given changeset and its corresponding GitDeltaManifestEntry
async fn packfile_entry(
    ctx: &CoreContext,
    repo: &impl Repo,
    delta_inclusion: DeltaInclusion,
    packfile_item_inclusion: PackfileItemInclusion,
    changeset_id: ChangesetId,
    path: MPath,
    mut entry: GitDeltaManifestEntry,
) -> Result<(PackfileItem, Option<ObjectId>)> {
    let blobstore = repo.repo_blobstore_arc();
    // Determine if the delta variant should be used or the base variant
    let delta = delta_base(&mut entry, delta_inclusion);
    match delta {
        Some(delta) => {
            let chunk_id_prefix =
                DeltaInstructionChunkIdPrefix::new(changeset_id, path.clone(), delta.origin, path);
            let instruction_bytes = fetch_delta_instructions(
                ctx,
                &blobstore,
                &chunk_id_prefix,
                delta.instructions_chunk_count,
            )
            .try_fold(
                BytesMut::with_capacity(delta.instructions_compressed_size as usize),
                |mut acc, bytes| async move {
                    acc.extend_from_slice(bytes.as_ref());
                    anyhow::Ok(acc)
                },
            )
            .await
            .context("Error in fetching delta instruction bytes from byte stream")?
            .freeze();

            let packfile_item = PackfileItem::new_delta(
                entry.full.oid,
                delta.base.oid,
                delta.instructions_uncompressed_size,
                instruction_bytes,
            );
            anyhow::Ok((packfile_item, Some(delta.base.oid)))
        }
        None => {
            // Use the full object instead
            let packfile_item = base_packfile_item(
                ctx,
                repo,
                ObjectIdentifierType::AllObjects(entry.full.as_rich_git_sha1()?),
                packfile_item_inclusion,
            )
            .await?;
            anyhow::Ok((packfile_item, None))
        }
    }
}

/// Fetch the stream of blob and tree objects as packfile items for the given changeset
async fn blob_and_tree_packfile_items<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    delta_inclusion: DeltaInclusion,
    packfile_item_inclusion: PackfileItemInclusion,
    changeset_id: ChangesetId,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
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
    let (mut items_in_pack, mut items_as_base) = (FxHashSet::default(), FxHashSet::default());
    let objects_stream = try_stream! {
        let mut entries = delta_manifest.into_subentries(ctx, &blobstore);
        while let Some((path, entry)) = entries.try_next().await? {
            // If the object is a duplicate OR if the object is already present at the client, then skip it
            if items_in_pack.contains(&entry.full.oid) || items_as_base.contains(&entry.full.oid) {
                continue;
            }
            items_in_pack.insert(entry.full.oid.clone());
            let (packfile_item, maybe_base) = packfile_entry(ctx, repo, delta_inclusion, packfile_item_inclusion, changeset_id, path, entry).await?;
            if let Some(delta_base) = maybe_base {
                if !items_as_base.contains(&delta_base) {
                    items_as_base.insert(delta_base);
                }
            }
            yield futures::future::ok(packfile_item)
        }
    };
    anyhow::Ok(objects_stream.try_buffered(500).boxed())
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn blob_and_tree_packfile_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    target_commits: BoxStream<'a, Result<ChangesetId>>,
    delta_inclusion: DeltaInclusion,
    packfile_item_inclusion: PackfileItemInclusion,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let packfile_item_stream = target_commits
        .map_ok(move |changeset_id| {
            blob_and_tree_packfile_items(
                ctx,
                repo,
                delta_inclusion,
                packfile_item_inclusion,
                changeset_id,
            )
        })
        .try_buffered(500)
        .try_flatten()
        .boxed();
    Ok(packfile_item_stream)
}

/// Create a stream of packfile items containing commit objects that need to be included in the packfile/bundle
async fn commit_packfile_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    target_commits: BoxStream<'a, Result<ChangesetId>>,
    packfile_item_inclusion: PackfileItemInclusion,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let commit_stream = target_commits
        .map_ok(move |changeset_id| async move {
            let maybe_git_sha1 = repo
                .bonsai_git_mapping()
                .get_git_sha1_from_bonsai(ctx, changeset_id)
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
                ctx,
                repo,
                ObjectIdentifierType::NonBlobObjects(git_objectid), // Since we know its not a blob
                packfile_item_inclusion,
            )
            .await
        })
        .try_buffered(500)
        .boxed();
    anyhow::Ok(commit_stream)
}

/// Convert the provided tag entries into a stream of packfile items
fn tag_entries_to_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    tag_entries: Vec<BonsaiTagMappingEntry>,
    packfile_item_inclusion: PackfileItemInclusion,
) -> BoxStream<'a, Result<PackfileItem>> {
    stream::iter(tag_entries.into_iter().map(anyhow::Ok))
        .map_ok(move |entry| async move {
            let git_objectid = entry.tag_hash.to_object_id()?;
            base_packfile_item(
                ctx,
                repo,
                ObjectIdentifierType::NonBlobObjects(git_objectid), // Since we know its not a blob
                packfile_item_inclusion,
            )
            .await
        })
        .try_buffered(500)
        .boxed()
}

/// Create a stream of packfile items containing tag objects that need to be included in the packfile/bundle while also
/// returning the total number of tags included in the stream
async fn tag_packfile_stream<'a>(
    ctx: &'a CoreContext,
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
    let packfile_item_inclusion = request.packfile_item_inclusion;
    let tag_stream = tag_entries_to_stream(ctx, repo, annotated_tags, packfile_item_inclusion);
    anyhow::Ok((tag_stream, tags_count))
}

/// Based on the input request parameters, generate a stream of `PackfileItem`s that
/// can be written into a pack file
pub async fn generate_pack_item_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    request: PackItemStreamRequest,
) -> Result<PackItemStreamResponse<'a>> {
    // We need to include the bookmarks (i.e. branches, tags) in the pack based on the request parameters
    let bookmarks = bookmarks(ctx, repo, &request.requested_refs)
        .await
        .with_context(|| {
            format!(
                "Error in fetching bookmarks for repo {}",
                repo.repo_identity().name()
            )
        })?;
    // Get all the commits that are reachable from the bookmarks
    let mut target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            ctx,
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
    let mut object_count = object_count(ctx, repo, to_commit_stream(target_commits.clone()))
        .await
        .context("Error while calculating object count")?;
    // Compute object count using the number of commit and content objects
    object_count += target_commits.len();

    // STEP 2: Create a mapping of all known bookmarks (i.e. branches, tags) and the commit that they point to. The commit should be represented
    // as a Git hash instead of a Bonsai hash since it will be part of the packfile/bundle
    let mut refs_to_include = refs_to_include(ctx, repo, &bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the pack")?;

    // STEP 2.5: Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(repo, request.requested_symrefs, &mut refs_to_include)
        .await
        .context("Error while adding symrefs to included set of refs")?;

    // STEP 3: Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let blob_and_tree_stream = blob_and_tree_packfile_stream(
        ctx,
        repo,
        to_commit_stream(target_commits.clone()),
        request.delta_inclusion,
        request.packfile_item_inclusion,
    )
    .await
    .context("Error while generating blob and tree packfile item stream")?;

    // STEP 4: Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let commit_stream = commit_packfile_stream(
        ctx,
        repo,
        to_commit_stream(target_commits.clone()),
        request.packfile_item_inclusion,
    )
    .await
    .context("Error while generating commit packfile item stream")?;

    // STEP 5: Get the stream of tag packfile items to include in the pack/bundle. Note that we have not yet included the tag count in the
    // total object count so we will need the stream + count of elements in the stream
    let (tag_stream, tags_count) = tag_packfile_stream(ctx, repo, &bookmarks, &request)
        .await
        .context("Error while generating tag packfile item stream")?;
    // Include the tags in the object count since the tags will also be part of the packfile/bundle
    object_count += tags_count;

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
