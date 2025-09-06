/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::sync::Arc;
use std::task::Poll;

use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use buffered_weighted::FutureWithWeight;
use buffered_weighted::GlobalWeight;
use buffered_weighted::MemoryBound;
use cloned::cloned;
use commit_graph_types::frontier::AncestorsWithinDistance;
use context::CoreContext;
use futures::StreamExt as _;
use futures::TryStreamExt;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::FuturesOrdered;
use futures_stats::TimedTryFutureExt;
use git_types::GitDeltaManifestEntryOps;
use git_types::GitIdentifier;
use git_types::GitTreeId;
use git_types::PackfileItem;
use git_types::fetch_git_delta_manifest;
use git_types::fetch_non_blob_git_object;
use git_types::tree::GitEntry;
use gix_hash::ObjectId;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;
use rustc_hash::FxHashSet;
use scuba_ext::FutureStatsScubaExt;
use scuba_ext::MononokeScubaSampleBuilder;
use tokio::sync::mpsc::Sender;

use crate::Repo;
use crate::bookmarks_provider::bookmarks;
use crate::bookmarks_provider::list_tags;
use crate::mapping::bonsai_git_mappings_by_bonsai;
use crate::mapping::git_shas_to_bonsais;
use crate::mapping::include_symrefs;
use crate::mapping::ordered_bonsai_git_mappings_by_bonsai;
use crate::mapping::refs_to_include;
use crate::store::ObjectIdentifierType;
use crate::store::base_packfile_item;
use crate::store::changeset_delta_manifest_entries;
use crate::store::packfile_item_for_delta_manifest_entry;
use crate::types::DeltaInclusion;
use crate::types::FetchContainer;
use crate::types::FetchRequest;
use crate::types::FetchResponse;
use crate::types::FullObjectEntry;
use crate::types::GitBookmarks;
use crate::types::LsRefsRequest;
use crate::types::LsRefsResponse;
use crate::types::PackItemStreamRequest;
use crate::types::PackItemStreamResponse;
use crate::types::PackfileItemInclusion;
use crate::types::RefsSource;
use crate::types::ShallowCommits;
use crate::types::ShallowInfoRequest;
use crate::types::ShallowInfoResponse;
use crate::types::ShallowVariant;
use crate::utils::ancestors_after_time;
use crate::utils::ancestors_excluding;
use crate::utils::commits;
use crate::utils::delta_base;
use crate::utils::entry_weight;
use crate::utils::filter_object;
use crate::utils::tag_entries_to_hashes;
use crate::utils::to_commit_stream;
use crate::utils::to_git_object_stream;

const DEFAULT_GIT_GENERATOR_BUFFER_BYTES: usize = 104_857_600; // 100 MB

/// Fetch and collect the tree and blob objects that are expressed as full objects
/// for the boundary commits of a shallow fetch
async fn boundary_trees_and_blobs(
    fetch_container: FetchContainer,
) -> Result<FxHashSet<FullObjectEntry>> {
    let FetchContainer {
        ctx,
        blobstore,
        filter,
        concurrency,
        shallow_info,
        ..
    } = fetch_container;
    let boundary_commits = match shallow_info.as_ref() {
        Some(shallow_info) => shallow_info.packfile_commits.boundary_commits.clone(),
        None => Vec::new(),
    };
    stream::iter(boundary_commits.into_iter().map(|entry| Ok((entry.csid(), entry.oid()))))
        .map_ok(async |(changeset_id, git_commit_id)| {
            let root_tree = fetch_non_blob_git_object(&ctx, &blobstore, &git_commit_id)
                .await
                .context("Error in fetching boundary commit")?
                .with_parsed_as_commit(|commit| GitTreeId(commit.tree()))
                .ok_or_else(|| anyhow::anyhow!("Git object {:?} is not a commit", git_commit_id))?;
            let objects = root_tree.list_all_entries((*ctx).clone(), blobstore.clone()).try_collect::<Vec<_>>().await?;
            let objects = stream::iter(objects).map(async |(path, entry)| {
                // If the entry is a submodule OR if the request has no filter or doesn't care about size, then let's assume size as 0
                let size = if entry.is_submodule() || filter.as_ref().as_ref().is_none_or(|filter| filter.no_size_constraint()) {
                    0
                } else {
                    entry.size(&ctx, &blobstore).await?
                };
                Ok((path, entry, size))
            })
            .buffer_unordered(concurrency.trees_and_blobs)
            .try_filter_map(async |(path, entry, size)| {
                let kind = entry.kind();
                let oid = entry.oid();
                // If the entry corresponds to a submodules (and shows up as a commit), then we ignore it
                // If the object is ignored by the filter, then we ignore it
                if !filter_object(filter.clone(), &path, kind, size) || entry.is_submodule() {
                    Ok(None)
                } else {
                    Ok(Some(FullObjectEntry::new(changeset_id, path, oid, size, kind)))
                }
            })
            .try_collect::<FxHashSet<_>>()
            .await
            .with_context(|| {
                format!(
                    "Error while listing all entries from GitTree for changeset {changeset_id:?} and root tree {root_tree:?}",
                )
            })?;
            Ok(objects)
        })
        .try_buffered(concurrency.shallow)
        .try_concat()
        .await
}

/// Get the count of distinct blob and tree items to be included in the packfile along with the
/// set of base objects that are expected to be present at the client
async fn trees_and_blobs_count(
    fetch_container: FetchContainer,
    target_commits: BoxStream<'_, Result<ChangesetId>>,
    explicitly_requested_objects: Vec<ObjectId>,
) -> Result<(usize, FxHashSet<ObjectId>)> {
    let FetchContainer {
        ctx,
        git_delta_manifest_version,
        delta_inclusion,
        derived_data,
        blobstore,
        filter,
        concurrency,
        chain_breaking_mode,
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
        .map_ok(async |changeset_id| {
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
                .into_entries(&ctx, &blobstore.boxed())
                .try_filter_map(async |entry| {
                    let (kind, size) = (entry.full_object_kind(), entry.full_object_size());
                    // If the entry does not pass the filter, then it should not be included in the count
                    if !filter_object(filter.clone(), entry.path(), kind, size) {
                        return Ok(None);
                    }
                    let delta = delta_base(
                        entry.as_ref(),
                        delta_inclusion,
                        filter.clone(),
                        chain_breaking_mode,
                    );
                    let output = (
                        entry.full_object_oid(),
                        delta.map(|delta| delta.base_object_oid()),
                    );
                    Ok(Some(output))
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
        })
        .try_buffered(concurrency.trees_and_blobs);
    let object_set = explicitly_requested_objects
        .into_iter()
        .collect::<FxHashSet<_>>();
    boundary_stream
        .chain(body_stream)
        .try_fold(
            (object_set, FxHashSet::default()),
            async |(mut object_set, mut base_set), objects_with_bases| {
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

async fn boundary_stream(
    fetch_container: FetchContainer,
) -> Result<BoxStream<'static, Result<(ChangesetId, Box<dyn GitDeltaManifestEntryOps + Send>)>>> {
    let objects = boundary_trees_and_blobs(fetch_container)
        .await?
        .into_iter()
        .map(|full_entry| {
            let cs_id = full_entry.cs_id;
            let path = full_entry.path.clone();
            let delta_manifest_entry: Box<dyn GitDeltaManifestEntryOps + Send> =
                Box::new((path, full_entry.into_delta_manifest_entry()));
            Ok((cs_id, delta_manifest_entry))
        });
    Ok(stream::iter(objects).boxed())
}

/// Creates a stream of packfile items for the given changesets
fn packfile_stream_from_changesets<'a>(
    fetch_container: FetchContainer,
    base_set: Arc<FxHashSet<ObjectId>>,
    cs_ids: Vec<ChangesetId>,
) -> BoxStream<'a, Result<PackfileItem>> {
    let FetchContainer {
        ctx,
        blobstore,
        derived_data,
        delta_inclusion,
        filter,
        concurrency,
        chain_breaking_mode,
        ..
    } = fetch_container.clone();

    // Finding the packfiles items for each commit requires calling two functions:
    // 1) changeset_delta_manifest_entries: ChangesetId -> Vec<(ChangesetId, MPath, dyn GitDeltaManifestEntry)>
    // 2) packfile_item_for_delta_manifest_entry: (ChangesetId, MPath, dyn GitDeltaManifestEntry) -> Option<PackfileItem>
    //
    // Because changeset_delta_manifest_entries returns multiple entries, creating a stream that chains these two functions using stream
    // combinators is not trivial, at least not without chaining multiple calls to `buffered` which is problematic because when the second
    // layer of buffering is full the first layer of buffering doesn't get polled until a future in the second layer completes.
    //
    // The implementation below is almost equivalent to using two layers of `buffered`, except that each time we poll the stream
    // we always poll the first layer (delta_manifest_entries_futures). Storing any completed future output in a `VecDeque`
    // buffer (delta_manifest_entries_buffer).

    let mut cs_ids = cs_ids.into_iter().collect::<VecDeque<_>>();
    let mut delta_manifest_entries_buffer = VecDeque::new();
    let mut delta_manifest_entries_futures = FuturesOrdered::new();
    let mut packfile_items_futures = FuturesOrdered::new();
    let max_buffer = justknobs::get_as::<usize>("scm/mononoke:git_generator_buffer_bytes", None)
        .unwrap_or(DEFAULT_GIT_GENERATOR_BUFFER_BYTES);
    let mut buffer_weight = GlobalWeight::new(max_buffer); // ~100 MB

    stream::poll_fn(move |cx| {
        if cs_ids.is_empty()
            && delta_manifest_entries_futures.is_empty()
            && delta_manifest_entries_buffer.is_empty()
            && packfile_items_futures.is_empty()
        {
            return Poll::Ready(None);
        }

        while delta_manifest_entries_futures.len() + delta_manifest_entries_buffer.len()
            < concurrency.trees_and_blobs
        {
            if let Some(cs_id) = cs_ids.pop_front() {
                delta_manifest_entries_futures.push_back(changeset_delta_manifest_entries(
                    ctx.clone(),
                    blobstore.clone(),
                    derived_data.clone(),
                    fetch_container.git_delta_manifest_version,
                    cs_id,
                ));
            } else {
                break;
            }
        }

        // Ensure that we don't poll `delta_manifest_entries_futures` if it's empty. Technically this
        // might not be necessary, but streams are not supposed to be polled again if they ever return
        // Poll::Ready(None) so let's be safe.
        if !delta_manifest_entries_futures.is_empty() {
            while let Poll::Ready(Some(entries)) =
                delta_manifest_entries_futures.poll_next_unpin(cx)
            {
                let entries = entries?;
                for entry in entries {
                    delta_manifest_entries_buffer.push_back(entry);
                }
            }
        }

        loop {
            if let Some((_, entry)) = delta_manifest_entries_buffer.front() {
                // If `packfile_items_futures` is empty, then we have to poll the next item regardless of the current memory usage to
                // ensure that the stream makes progress
                if !packfile_items_futures.is_empty() {
                    let weight = entry_weight(
                        entry.as_ref(),
                        delta_inclusion,
                        filter.clone(),
                        chain_breaking_mode,
                    );
                    // If the next future will tip memory usage over the memory bound OR if we don't have enough buffer budget for it,
                    // then don't start polling it
                    if !buffer_weight.has_space_for(weight)
                        || !MemoryBound::new(Some(concurrency.memory_bound)).within_bound(weight)
                    {
                        break;
                    }
                }
            }

            if let Some((_cs_id, entry)) = delta_manifest_entries_buffer.pop_front() {
                let weight = entry_weight(
                    entry.as_ref(),
                    delta_inclusion,
                    filter.clone(),
                    chain_breaking_mode,
                );
                buffer_weight.add_weight(weight);
                let fut = packfile_item_for_delta_manifest_entry(
                    fetch_container.clone(),
                    base_set.clone(),
                    entry,
                );
                packfile_items_futures.push_back(FutureWithWeight::new(weight, fut));
            } else {
                break;
            }
        }
        // If none of the delta_manifest_entries_futures have completed, then its possible that packfile_item_futures is empty. If we return
        // packfile_items_futures.poll_next_unpin() in that case then we will end up returning Poll::Ready(None) and the stream will never get
        // polled again even though there are still items to be processed.
        if packfile_items_futures.is_empty() {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            match packfile_items_futures.poll_next_unpin(cx) {
                Poll::Ready(Some((weight, output))) => {
                    buffer_weight.sub_weight(weight);
                    Poll::Ready(Some(output))
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }
    })
    .try_filter_map(futures::future::ok)
    .boxed()
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn tree_and_blob_packfile_stream<'a>(
    fetch_container: FetchContainer,
    target_commits: Vec<ChangesetId>,
    base_set: Arc<FxHashSet<ObjectId>>,
    tree_and_blob_shas: Vec<ObjectId>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let FetchContainer {
        ctx,
        blobstore,
        concurrency,
        packfile_item_inclusion,
        ..
    } = fetch_container.clone();

    let boundary_packfile_item_stream = boundary_stream(fetch_container.clone())
        .await?
        .map_ok({
            cloned!(fetch_container, base_set);
            move |(_changeset_id, entry)| {
                cloned!(fetch_container, base_set);
                async move {
                    packfile_item_for_delta_manifest_entry(fetch_container, base_set, entry).await
                }
            }
        })
        .try_buffered(concurrency.trees_and_blobs)
        .try_filter_map(futures::future::ok)
        .boxed();

    let packfile_item_stream =
        packfile_stream_from_changesets(fetch_container, base_set, target_commits);

    let requested_trees_and_blobs = stream::iter(tree_and_blob_shas.into_iter().map(Ok))
        .map_ok(move |oid| {
            cloned!(ctx, blobstore);
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
        Some(shallow_info) => shallow_info
            .packfile_commits
            .boundary_commits
            .iter()
            .map(|entry| entry.csid())
            .collect(),
        None => Vec::new(),
    };
    commit_count += shallow_commits.len();
    let final_commits = [target_commits, shallow_commits].concat();
    let git_commits = bonsai_git_mappings_by_bonsai(&ctx, repo, final_commits)
        .await?
        .into_values()
        .collect::<Vec<_>>();
    let commit_stream = to_git_object_stream(git_commits)
        .map_ok(move |git_objectid| {
            let blobstore = blobstore.clone();
            let ctx = ctx.clone();
            async move {
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
    tag_entries: FxHashSet<GitSha1>,
) -> BoxStream<'a, Result<PackfileItem>> {
    let FetchContainer {
        ctx,
        blobstore,
        packfile_item_inclusion,
        concurrency,
        ..
    } = fetch_container;
    stream::iter(tag_entries.into_iter().map(Ok))
        .map_ok(move |tag_hash| {
            let blobstore = blobstore.clone();
            let ctx = ctx.clone();
            async move {
                let git_objectid = tag_hash.to_object_id()?;
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
    ctx: &CoreContext,
    fetch_container: FetchContainer,
    repo: &'a impl Repo,
    bookmarks: &GitBookmarks,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    // Since we need the count of items, we would have to consume the stream either for counting or collecting the items.
    // This is fine, since unlike commits, blobs and trees there will only be thousands of tags in the worst case.
    let annotated_tags = stream::iter(bookmarks.entries.keys())
        .filter_map(async |bookmark| {
            // If the bookmark is actually a tag but there is no mapping in bonsai_tag_mapping table for it, then it
            // means that its a simple tag and won't be included in the packfile as an object. If a mapping exists, then
            // it will be included in the packfile as a raw Git

            // NOTE: There is no need to check if the bookmark is a tag. If its present in bonsai_tag_mapping table, then it
            // is an annotated tag
            let tag_name = bookmark.name().to_string();
            repo.bonsai_tag_mapping()
                .get_entry_by_tag_name(
                    ctx,
                    tag_name.clone(),
                    bonsai_tag_mapping::Freshness::MaybeStale,
                )
                .await
                .with_context(|| {
                    format!(
                        "Error in getting bonsai_tag_mapping entry for tag name {}",
                        tag_name
                    )
                })
                .transpose()
        })
        .try_collect::<Vec<_>>()
        .await?;
    let annotated_tags = tag_entries_to_hashes(
        annotated_tags,
        fetch_container.ctx.clone(),
        fetch_container.blobstore.clone(),
        fetch_container.concurrency.tags,
    )
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
    refs_source: RefsSource,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    let (ctx, filter, blobstore, concurrency) = (
        fetch_container.ctx.clone(),
        fetch_container.filter.clone(),
        fetch_container.blobstore.clone(),
        fetch_container.concurrency,
    );
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
        true => list_tags(&ctx, repo, refs_source)
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
            .context("Error in getting tags pointing to input set of commits")?,
        false => FxHashSet::default(),
    };
    // Fetch entries corresponding to annotated tags in the repo or with names
    // that match the requested tag names
    let tag_entries = repo
        .bonsai_tag_mapping()
        .get_all_entries(&ctx)
        .await
        .context("Error in getting tags during fetch")?
        .into_iter()
        .filter(|entry| {
            required_tag_names.contains(&entry.tag_name)
                || requested_tag_names.contains(&entry.tag_name)
        })
        .collect::<Vec<_>>();
    let exhaustive_tag_entries =
        tag_entries_to_hashes(tag_entries, ctx, blobstore, concurrency.tags).await?;

    let tags_count = exhaustive_tag_entries.len();
    let tag_stream = tag_entries_to_stream(fetch_container, exhaustive_tag_entries);
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
    let bookmarks = bookmarks(&ctx, repo, &request.requested_refs, request.refs_source)
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
        request.chain_breaking_mode,
    )?;
    // Get all the commits that are reachable from the bookmarks
    let mut target_commits = repo
        .commit_graph()
        .ancestors_difference_stream(
            &ctx,
            bookmarks.entries.values().copied().collect(),
            request.have_heads.clone(),
        )
        .await
        .context("Error in getting ancestors difference while generating packitem stream")?
        .try_collect::<Vec<_>>()
        .await?;
    let mut bookmarks = bookmarks
        .clone()
        .try_into_git_bookmarks(&ctx, repo)
        .await
        .context("Error while converting bookmarks to Git format during upload-pack")?;
    let bookmarks = bookmarks
        .with_content_refs(ctx.as_ref(), repo)
        .await
        .context("Error while getting content refs during upload-pack")?;
    // Reverse the list of commits so that we can prevent delta cycles from appearing in the packfile
    target_commits.reverse();
    // STEP 1: Get the count of distinct blob and tree objects to be included in the packfile/bundle.
    let (trees_and_blobs_count, base_set) = trees_and_blobs_count(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
        vec![],
    )
    .await
    .context("Error while calculating object count")?;

    // STEP 2: Create a mapping of all known bookmarks (i.e. branches, tags) and the commit that they point to. The commit should be represented
    // as a Git hash instead of a Bonsai hash since it will be part of the packfile/bundle
    let mut refs_to_include = refs_to_include(ctx.as_ref(), repo, bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the pack")?;

    // STEP 2.5: Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(&ctx, repo, request.requested_symrefs, &mut refs_to_include)
        .await
        .context("Error while adding symrefs to included set of refs")?;

    // STEP 3: Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let tree_and_blob_stream = tree_and_blob_packfile_stream(
        fetch_container.clone(),
        target_commits.clone(),
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
    let (tag_stream, tags_count) =
        tag_packfile_stream(ctx.as_ref(), fetch_container.clone(), repo, bookmarks)
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
    let mut bookmarks = bookmarks(ctx, repo, &request.requested_refs, request.refs_source)
        .await
        .with_context(|| {
            format!(
                "Error in fetching bookmarks for repo {}",
                repo.repo_identity().name()
            )
        })?
        .try_into_git_bookmarks(ctx, repo)
        .await
        .context("Error while converting bookmarks to Git format during ls-refs")?;
    let bookmarks = bookmarks
        .with_content_refs(ctx, repo)
        .await
        .context("Error while getting content refs during ls-refs")?;
    // Convert the above bookmarks into refs that can be sent in the response
    let mut refs_to_include = refs_to_include(ctx, repo, bookmarks, request.tag_inclusion)
        .await
        .context("Error while determining refs to include in the response")?;

    // Add symrefs to the refs_to_include map based on the request parameters
    include_symrefs(ctx, repo, request.requested_symrefs, &mut refs_to_include)
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
    progress_writer: Sender<String>,
    perf_scuba: MononokeScubaSampleBuilder,
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
        request.chain_breaking_mode,
    )?;
    // Convert the base commits and head commits, which are represented as Git hashes, into Bonsai hashes
    // If the input contains tag object Ids, fetch the corresponding tag names
    progress_writer
        .send("Converting HAVE Git commits to Bonsais\n".to_string())
        .await?;
    let translated_sha_bases = git_shas_to_bonsais(&ctx, repo, request.bases.iter())
        .try_timed()
        .await
        .context("Error converting base Git commits to Bonsai during fetch")?
        .log_future_stats(
            perf_scuba.clone(),
            "Converted HAVE Git commits to Bonsais",
            "Read".to_string(),
        );
    progress_writer
        .send("Converting WANT Git commits to Bonsais\n".to_string())
        .await?;
    let translated_sha_heads = git_shas_to_bonsais(&ctx, repo, request.heads.iter())
        .try_timed()
        .await
        .context("Error converting head Git commits to Bonsai during fetch")?
        .log_future_stats(
            perf_scuba.clone(),
            "Converted WANT Git commits to Bonsais",
            "Read".to_string(),
        );
    // Get the stream of commits between the bases and heads
    // NOTE: Another Git magic. The filter spec includes an option that the client can use to exclude commit-type objects. But, even if the client
    // uses that filter, we just ignore it and send all the commits anyway :)
    progress_writer
        .send("Collecting Bonsai commits to send to client\n".to_string())
        .await?;
    let mut target_commits = commits(
        &ctx,
        repo,
        translated_sha_heads.bonsais.clone(),
        translated_sha_bases.bonsais.clone(),
        &shallow_info,
    )
    .try_timed()
    .await?
    .log_future_stats(
        perf_scuba.clone(),
        "Collected Bonsai commits to send to client",
        "Read".to_string(),
    );
    // Reverse the list of commits so that we can prevent delta cycles from appearing in the packfile
    target_commits.reverse();
    progress_writer
        .send("Counting number of objects to be sent in packfile\n".to_string())
        .await?;
    // Get the count of unique blob and tree objects to be included in the packfile
    let (trees_and_blobs_count, base_set) = trees_and_blobs_count(
        fetch_container.clone(),
        to_commit_stream(target_commits.clone()),
        translated_sha_heads.non_tag_non_commit_oids.clone(),
    )
    .try_timed()
    .await
    .context("Error while calculating object count during fetch")?
    .log_future_stats(
        perf_scuba.clone(),
        "Counted number of objects to be sent in packfile",
        "Read".to_string(),
    );
    // Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    progress_writer
        .send("Generating trees and blobs stream\n".to_string())
        .await?;
    let tree_and_blob_stream = tree_and_blob_packfile_stream(
        fetch_container.clone(),
        target_commits.clone(),
        Arc::new(base_set),
        translated_sha_heads.non_tag_non_commit_oids,
    )
    .try_timed()
    .await
    .context("Error while generating blob and tree packfile item stream during fetch")?
    .log_future_stats(
        perf_scuba.clone(),
        "Generated trees and blobs stream",
        "Read".to_string(),
    );
    // Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    progress_writer
        .send("Generating commits stream\n".to_string())
        .await?;
    let (commit_stream, commits_count) =
        commit_packfile_stream(fetch_container.clone(), repo, target_commits.clone())
            .try_timed()
            .await
            .context("Error while generating commit packfile item stream during fetch")?
            .log_future_stats(
                perf_scuba.clone(),
                "Generated commits stream",
                "Read".to_string(),
            );
    // Get the stream of all annotated tag items in the repo
    progress_writer
        .send("Generating tags stream\n".to_string())
        .await?;
    let (tag_stream, tags_count) = tags_packfile_stream(
        fetch_container,
        repo,
        target_commits,
        translated_sha_heads.tag_names.clone(),
        request.refs_source,
    )
    .try_timed()
    .await
    .context("Error while generating tag packfile item stream during fetch")?
    .log_future_stats(
        perf_scuba.clone(),
        "Generated tags stream",
        "Read".to_string(),
    );
    // Combine all streams together and return the response. The ordering of the streams in this case is irrelevant since the commit
    // and tag stream include full objects and the tree_and_blob_stream has deltas in the correct order
    let packfile_stream = tag_stream
        .chain(commit_stream)
        .chain(tree_and_blob_stream)
        .boxed();
    progress_writer
        .send("Sending packfile stream\n".to_string())
        .await?;
    Ok(FetchResponse::new(
        packfile_stream,
        commits_count,
        trees_and_blobs_count,
        tags_count,
    ))
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
    // Convert the provided have object ids to bonsais so that we can use Mononoke commit graph
    let translated_sha_bases = git_shas_to_bonsais(&ctx, repo, request.bases.iter())
        .await
        .context("Error converting base Git commits to Bonsai during shallow-info")?;
    let shallow_commits = ordered_bonsai_git_mappings_by_bonsai(
        &ctx,
        repo,
        translated_shallow_commits.bonsais.clone(),
    )
    .await
    .context("Error fetching Git mappings for shallow bonsais")?;
    let ancestors_within_distance = match &request.variant {
        ShallowVariant::FromServerWithDepth(depth) => repo
            .commit_graph()
            .ancestors_within_distance(&ctx, translated_sha_heads.bonsais, (*depth - 1) as u64)
            .await
            .context("Error in getting ancestors within distance from heads commits during shallow-info")?,
        ShallowVariant::FromClientWithDepth(depth) => repo
            .commit_graph()
            .ancestors_within_distance(&ctx, translated_shallow_commits.bonsais.clone(), *depth as u64)
            .await
            .context("Error in getting ancestors within distance from shallow commits during shallow-info")?,
        ShallowVariant::FromServerWithTime(time) => ancestors_after_time(&ctx, repo, translated_sha_heads.bonsais, *time)
            .await
            .context("Error in getting ancestors after time during shallow-info")?,
        ShallowVariant::FromServerExcludingRefs(excluded_refs) => ancestors_excluding(&ctx, repo, translated_sha_heads.bonsais, excluded_refs.clone())
            .await
            .context("Error in getting ancestors excluding refs during shallow-info")?,
        ShallowVariant::None => AncestorsWithinDistance::default(),
    };
    // We might decide not to send some objects based on the client's HAVES and SHALLOW but for reporting purposes in shallow section
    // of Git protocol, we need to provide visibility into all eligible commits. That's the purpose of info_commits
    let info_commits = ShallowCommits {
        commits: ordered_bonsai_git_mappings_by_bonsai(
            &ctx,
            repo,
            ancestors_within_distance.ancestors.clone(),
        )
        .await
        .context("Error fetching Git mappings for boundary bonsais")?,
        boundary_commits: ordered_bonsai_git_mappings_by_bonsai(
            &ctx,
            repo,
            ancestors_within_distance.boundaries.clone(),
        )
        .await
        .context("Error fetching Git mappings for boundary bonsais")?,
    };
    // Get the set of commits that are already present at the client so we don't resend them as part of this fetch
    let mut client_commits = repo
        .commit_graph()
        .ancestors_difference(
            &ctx,
            translated_sha_bases.bonsais,
            translated_shallow_commits.bonsais.clone(),
        )
        .await
        .context("Error in fetching difference of ancestors between client haves and shallow")?
        .into_iter()
        .collect::<FxHashSet<_>>();
    client_commits.extend(translated_shallow_commits.bonsais);
    let boundaries = ancestors_within_distance
        .boundaries
        .into_iter()
        .filter(|commit| !client_commits.contains(commit))
        .collect();
    let ancestors = ancestors_within_distance
        .ancestors
        .into_iter()
        .filter(|commit| !client_commits.contains(commit))
        .collect();

    let boundary_commits = ordered_bonsai_git_mappings_by_bonsai(&ctx, repo, boundaries)
        .await
        .context("Error fetching Git mappings for boundary bonsais")?;
    let target_commits = ordered_bonsai_git_mappings_by_bonsai(&ctx, repo, ancestors)
        .await
        .context("Error fetching Git mappings for target bonsais")?;
    let packfile_commits = ShallowCommits {
        commits: target_commits,
        boundary_commits,
    };
    Ok(ShallowInfoResponse::new(
        packfile_commits,
        info_commits,
        shallow_commits,
    ))
}
