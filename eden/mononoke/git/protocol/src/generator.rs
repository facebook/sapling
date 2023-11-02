/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_stream::try_stream;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::BytesMut;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_symbolic_refs::GitSymbolicRefsRef;
use git_types::fetch_delta_instructions;
use git_types::fetch_git_object;
use git_types::get_object_bytes;
use git_types::DeltaInstructionChunkIdPrefix;
use git_types::GitDeltaManifestEntry;
use git_types::RootGitDeltaManifestId;
use gix_hash::ObjectId;
use gix_object::WriteTo;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use packfile::types::PackfileItem;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::types::DeltaInclusion;
use crate::types::PackItemStreamRequest;
use crate::types::PackItemStreamResponse;
use crate::types::RequestedRefs;
use crate::types::RequestedSymrefs;
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
    request: &PackItemStreamRequest,
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
            let refs = request.requested_refs.clone();
            let name = bookmark.name().to_string();
            async move {
                let result = match refs {
                    RequestedRefs::Included(refs) if refs.contains(&name) => {
                        Some((bookmark.into_key(), cs_id))
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
    if let RequestedRefs::IncludedWithValue(ref ref_value_map) = request.requested_refs {
        for (ref_name, ref_value) in ref_value_map {
            bookmarks.insert(
                BookmarkKey::with_name(ref_name.as_str().try_into()?),
                ref_value.clone(),
            );
        }
    }
    Ok(bookmarks)
}

/// Get the count of tree, blob and commit objects that will be included in the packfile/bundle
/// by summing up the entries in the delta manifest for each commit that is to be included. Also
/// add the count of commits for which the delta manifests are being explored. This method also
/// returns the set of objects that are duplicated atleast once across multiple commits.
async fn object_count(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    request: &PackItemStreamRequest,
) -> Result<(usize, FxHashSet<ObjectId>)> {
    // Get all the commits that are reachable from the bookmarks
    let target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            ctx,
            bookmarks.values().copied().collect(),
            request.have_heads.clone(),
        )
        .await
        .context("Error in getting ancestors difference")?;
    // Sum up the entries in the delta manifest for each commit included in packfile
    let (unique_objects, duplicate_objects, commit_count) = target_commits
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
        .try_buffered(100)
        .try_fold(
            (FxHashSet::default(), // The set of all unique objects to be included in the pack file
                  FxHashSet::default(), // The set of objects that have repeated atleast once
                  0), // The number of commits whose delta manifests are being explored
            |(mut unique_objects, mut duplicate_objects, commit_count), objects_in_entry| async move {
                for entry in objects_in_entry.into_iter() {
                    if unique_objects.contains(&entry) {
                        duplicate_objects.insert(entry);
                    } else {
                        unique_objects.insert(entry);
                    }
                }
                // The +1 is to account for the commit itself which will also be included as
                // part of the packfile/bundle
                anyhow::Ok((unique_objects, duplicate_objects, commit_count + 1))
            },
        )
        .await?;
    // The total object count is the count of unique blob and tree objects + the count of commits objects
    // in the range
    let total_object_count = unique_objects.len() + commit_count;
    Ok((total_object_count, duplicate_objects))
}

/// Get the list of Git refs that need to be included in the stream of PackfileItem. On Mononoke end, this
/// will be bookmarks created from branches and tags. Branches and simple tags will be mapped to the
/// Git commit that they point to. Annotated tags will be handled based on the `tag_inclusion` parameter
async fn refs_to_include(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    tag_inclusion: TagInclusion,
) -> Result<FxHashMap<String, ObjectId>> {
    stream::iter(bookmarks.iter())
        .map(|(bookmark, cs_id)| async move {
            if bookmark.is_tag() && tag_inclusion == TagInclusion::AsIs {
                let tag_name = bookmark.name().to_string();
                let entry = repo
                    .bonsai_tag_mapping()
                    .get_entry_by_tag_name(tag_name.clone())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in gettting bonsai_tag_mapping entry for tag name {}",
                            tag_name
                        )
                    })?;
                if let Some(entry) = entry {
                    let git_objectid = entry.tag_hash.to_object_id()?;
                    let ref_name = format!("refs/{}", bookmark);
                    return anyhow::Ok((ref_name, git_objectid));
                }
            };
            let maybe_git_sha1 = repo
                .bonsai_git_mapping()
                .get_git_sha1_from_bonsai(ctx, *cs_id)
                .await
                .with_context(|| {
                    format!(
                        "Error in fetching Git Sha1 for changeset {:?} through BonsaiGitMapping",
                        cs_id
                    )
                })?;
            let git_sha1 = maybe_git_sha1
                .ok_or_else(|| anyhow::anyhow!("Git Sha1 not found for changeset {:?}", cs_id))?;
            let git_objectid =
                ObjectId::from_hex(git_sha1.to_hex().as_bytes()).with_context(|| {
                    format!(
                        "Error in converting GitSha1 {:?} to GitObjectId",
                        git_sha1.to_hex()
                    )
                })?;
            let ref_name = format!("refs/{}", bookmark);
            anyhow::Ok((ref_name, git_objectid))
        })
        .boxed()
        .buffer_unordered(100)
        .try_collect::<FxHashMap<_, _>>()
        .await
}

/// The HEAD ref in Git doesn't have a direct counterpart in Mononoke bookmarks and is instead
/// stored in the git_symbolic_refs. Fetch the mapping and add them to the list of refs to include
async fn include_symrefs(
    repo: &impl Repo,
    requested_symrefs: RequestedSymrefs,
    refs_to_include: &mut FxHashMap<String, ObjectId>,
) -> Result<()> {
    let symref_commit_mapping = match requested_symrefs {
        RequestedSymrefs::IncludeHead => {
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
                })?;
            FxHashMap::from_iter([(head_ref.symref_name, head_commit_id.clone())])
        }
        RequestedSymrefs::IncludeAll => {
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
                    })?;
                Ok((entry.symref_name, ref_commit_id.clone()))
            }).collect::<Result<FxHashMap<_, _>>>()?
        }
        RequestedSymrefs::ExcludeAll => FxHashMap::default(),
    };

    // Add the symref -> commit mapping to the refs_to_include map
    refs_to_include.extend(symref_commit_mapping.into_iter());
    Ok(())
}

/// Generate a PackfileEntry for the given changeset and its corresponding GitDeltaManifestEntry
async fn packfile_entry(
    ctx: &CoreContext,
    repo: &impl Repo,
    delta_inclusion: DeltaInclusion,
    changeset_id: ChangesetId,
    path: MPath,
    mut entry: GitDeltaManifestEntry,
    is_duplicated: bool,
) -> Result<PackfileItem> {
    let blobstore = repo.repo_blobstore_arc();
    // Determine if the delta variant should be used or the base variant
    let use_delta = match delta_inclusion {
        DeltaInclusion::Include {
            inclusion_threshold,
            ..
        } => {
            // Can't use the delta if no delta variant is present in the entry. Additionally, if this object has been
            // duplicated across multiple commits in the pack, then we can't use it as a delta due to the potential of
            // a delta cycle
            let mut use_delta = entry.is_delta() && !is_duplicated;
            // Get the delta with the shortest size. In case of shallow clones, we would also want to validate if the
            // base of the delta is present in the pack or at the client.
            // TODO(rajshar): Implement delta support in shallow clones
            entry.deltas.sort_by(|a, b| {
                a.instructions_compressed_size
                    .cmp(&b.instructions_compressed_size)
            });
            let shortest_delta = entry.deltas.first();
            // Only use the delta if the size of the delta is less than inclusion_threshold% the size of the actual object
            use_delta &= shortest_delta.map_or(false, |delta| {
                (delta.instructions_compressed_size as f64)
                    < (entry.full.size as f64) * inclusion_threshold as f64
            });
            use_delta
        }
        // Can't use the delta variant if the request prevents us from using it
        DeltaInclusion::Exclude => false,
    };
    if use_delta {
        // Use the delta variant
        let delta = entry.deltas.first().unwrap(); // Should have a value by this point
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
        anyhow::Ok(packfile_item)
    } else {
        // Use the full object instead
        let bytes = get_object_bytes(
            ctx,
            blobstore.clone(),
            &entry.full,
            &entry.full.as_rich_git_sha1()?,
            git_types::HeaderState::Included,
        )
        .await
        .context("Error in fetching git object bytes from byte stream")?;
        let packfile_item = PackfileItem::new_base(bytes).with_context(|| {
            format!(
                "Error in creating packfile item from git object bytes for {:?}",
                &entry.full.oid
            )
        })?;
        anyhow::Ok(packfile_item)
    }
}

/// Fetch the stream of blob and tree objects as packfile items for the given changeset
async fn blob_and_tree_packfile_items<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    delta_inclusion: DeltaInclusion,
    changeset_id: ChangesetId,
    duplicated_objects: Arc<FxHashSet<ObjectId>>,
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
    let objects_stream = try_stream! {
        let mut entries = delta_manifest.into_subentries(ctx, &blobstore);
        while let Some((path, entry)) = entries.try_next().await? {
            let is_duplicated = duplicated_objects.contains(&entry.full.oid);
            let packfile_item = packfile_entry(ctx, repo, delta_inclusion, changeset_id, path, entry, is_duplicated).await?;
            yield packfile_item
        }
    };
    anyhow::Ok(objects_stream.boxed())
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn blob_and_tree_packfile_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    request: &PackItemStreamRequest,
    duplicated_objects: FxHashSet<ObjectId>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            ctx,
            bookmarks.values().copied().collect(),
            request.have_heads.clone(),
        )
        .await
        .context("Error in getting ancestors difference")?;

    // If the output stream can contain only offset deltas, then the commits must be ordered from root to
    // head since the base of each delta should appear before the delta object. The ancestors difference
    // stream will return the commits in head to root order so we need to reverse it. Note that this impacts
    // performance and forces us to hold the entire commit range in memory.
    let target_commits = if request.delta_inclusion.include_only_offset_deltas() {
        let mut collected_commits = target_commits.try_collect::<Vec<_>>().await?;
        collected_commits.reverse();
        stream::iter(collected_commits.into_iter().map(anyhow::Ok)).boxed()
    } else {
        target_commits
    };

    let delta_inclusion = request.delta_inclusion;
    let duplicated_objects = Arc::new(duplicated_objects);
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let packfile_item_stream = target_commits
        .and_then(move |changeset_id| {
            blob_and_tree_packfile_items(
                ctx,
                repo,
                delta_inclusion,
                changeset_id,
                duplicated_objects.clone(),
            )
        })
        .try_flatten()
        .boxed();
    Ok(packfile_item_stream)
}

/// Create a stream of packfile items containing commit objects that need to be included in the packfile/bundle
async fn commit_packfile_stream<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmarks: &FxHashMap<BookmarkKey, ChangesetId>,
    request: &PackItemStreamRequest,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            ctx,
            bookmarks.values().copied().collect(),
            request.have_heads.clone(),
        )
        .await
        .context("Error in getting ancestors difference")?;
    let commit_stream = target_commits
        .and_then(move |changeset_id| async move {
            let blobstore = repo.repo_blobstore_arc();
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
            let object = fetch_git_object(ctx, &blobstore, git_objectid.as_ref()).await?;
            let mut object_bytes = object.loose_header().into_vec();
            object.write_to(object_bytes.by_ref())?;
            let packfile_item = PackfileItem::new_base(object_bytes.into()).with_context(|| {
                format!(
                    "Error in creating packfile item from git object bytes for {:?}",
                    &object
                )
            })?;
            anyhow::Ok(packfile_item)
        })
        .boxed();
    anyhow::Ok(commit_stream)
}

/// Create a stream of packfile items containing tag objects that need to be included in the packfile/bundle while also
/// returning the total number of tags included in the stream
async fn tag_packfile_stream<'a>(
    ctx: &'a CoreContext,
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
    let tag_stream = stream::iter(annotated_tags.into_iter().map(anyhow::Ok))
        .and_then(move |entry| async move {
            let blobstore = repo.repo_blobstore_arc();
            let git_objectid = entry.tag_hash.to_object_id()?;
            let object = fetch_git_object(ctx, &blobstore, git_objectid.as_ref()).await?;
            let mut object_bytes = object.loose_header().into_vec();
            object.write_to(object_bytes.by_ref())?;
            let packfile_item = PackfileItem::new_base(object_bytes.into()).with_context(|| {
                format!(
                    "Error in creating packfile item from git object bytes for {:?}",
                    &object
                )
            })?;
            anyhow::Ok(packfile_item)
        })
        .boxed();
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
    let bookmarks = bookmarks(ctx, repo, &request).await.with_context(|| {
        format!(
            "Error in fetching bookmarks for repo {}",
            repo.repo_identity().name()
        )
    })?;

    // STEP 1: Create state to track the total number of objects that will be included in the packfile/bundle. Initialize with the
    // tree, blob and commit count. Collect the set of duplicated objects.
    let (mut object_count, duplicated_objects) = object_count(ctx, repo, &bookmarks, &request)
        .await
        .context("Error while counting objects for packing")?;

    // STEP 2: Create a mapping of all known bookmarks (i.e. branches, tags) and the commit that they point to. The commit should be represented
    // as a Git hash instead of a Bonsai hash since it will be part of the packfile/bundle
    let mut refs_to_include = refs_to_include(ctx, repo, &bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the pack")?;

    // STEP 2.5: Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(repo, request.requested_symrefs, &mut refs_to_include)
        .await
        .context("Error while adding HEAD ref to included set of refs")?;

    // STEP 3: Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let blob_and_tree_stream =
        blob_and_tree_packfile_stream(ctx, repo, &bookmarks, &request, duplicated_objects)
            .await
            .context("Error while generating blob and tree packfile item stream")?;

    // STEP 4: Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let commit_stream = commit_packfile_stream(ctx, repo, &bookmarks, &request)
        .await
        .context("Error while generating commit packfile item stream")?;

    // STEP 5: Get the stream of tag packfile items to include in the pack/bundle. Note that we have not yet included the tag count in the
    // total object count so we will need the stream + count of elements in the stream
    let (tag_stream, tags_count) = tag_packfile_stream(ctx, repo, &bookmarks)
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
