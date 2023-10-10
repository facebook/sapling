/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_stream::try_stream;
use blobstore::Loadable;
use bookmarks::Bookmark;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Freshness;
use bytes::Bytes;
use bytes::BytesMut;
use clap::Args;
use context::CoreContext;
use flate2::write::ZlibDecoder;
use futures::stream;
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
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
use packfile::bundle::BundleWriter;
use packfile::pack::DeltaForm;
use packfile::types::PackfileItem;
use repo_blobstore::RepoBlobstoreArc;
use walkdir::WalkDir;

use super::Repo;

const HEAD_REF_PREFIX: &str = "ref: ";
const NON_OBJECTS_DIR: [&str; 2] = ["info", "pack"];
const OBJECTS_DIR: &str = "objects";
const REFS_DIR: &str = "refs";
const HEAD_REF: &str = "HEAD";

#[derive(Args)]
/// Arguments for creating a Git bundle
pub struct CreateBundleArgs {
    /// The location, i.e. file_name + path, where the generated bundle will be stored
    #[clap(long, short = 'o', value_name = "FILE")]
    output_location: PathBuf,
    /// The path to the Git repo where the required objects to be bundled are present
    /// e.g. /repo/path/.git. If not provided, the command will use the Mononoke repo
    /// for creating the bundle instead.
    #[clap(long, value_name = "FILE")]
    git_repo_path: Option<PathBuf>,
}

pub async fn create(ctx: &CoreContext, create_args: CreateBundleArgs, repo: Repo) -> Result<()> {
    // Open the output file for writing
    let output_file = tokio::fs::File::create(create_args.output_location.as_path())
        .await
        .with_context(|| {
            format!(
                "Error in opening/creating output file {}",
                create_args.output_location.display()
            )
        })?;
    match create_args.git_repo_path {
        Some(path) => create_from_on_disk_repo(path, output_file).await,
        None => create_from_mononoke_repo(ctx, repo, output_file).await,
    }
}

/// Get all the bookmarks (branches, tags) and their corresponding commits
/// for the given repo
async fn all_bookmarks(repo: &Repo, ctx: &CoreContext) -> Result<HashMap<Bookmark, ChangesetId>> {
    repo.bookmarks
        .list(
            ctx.clone(),
            Freshness::MostRecent,
            &BookmarkPrefix::empty(),
            BookmarkCategory::ALL,
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            u64::MAX,
        )
        .try_collect::<HashMap<_, _>>()
        .await
}

/// Get the count of tree, blob and commit objects that will be included in the packfile/bundle
/// by summing up the entries in the delta manifest for each commit that is to be included. Also
/// add the count of commits for which the delta manifests are being explored. This method also
/// returns the set of objects that are duplicated atleast once across multiple commits.
async fn object_count(
    repo: &Repo,
    ctx: &CoreContext,
    bookmarks: &HashMap<Bookmark, ChangesetId>,
) -> Result<(usize, HashSet<ObjectId>)> {
    // Get all the commits that are reachable from the bookmarks
    let target_commits = repo
        .commit_graph
        .ancestors_difference_stream(ctx, bookmarks.values().copied().collect(), vec![])
        .await
        .context("Error in getting ancestors difference")?;
    // Sum up the entries in the delta manifest for each commit included in packfile
    let (unique_objects, duplicate_objects, commit_count) = target_commits
        .map_ok(|changeset_id| {
            async move {
                let blobstore = repo.repo_blobstore_arc();
                let root_mf_id = repo
                    .repo_derived_data
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
                // Get the hashset of the tree and blob object Ids that will be included
                // in the packfile
                let objects = delta_manifest
                    .into_subentries(ctx, &blobstore)
                    .map_ok(|(_, entry)| entry.full.oid)
                    .try_collect::<HashSet<_>>()
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
            (HashSet::new(), // The set of all unique objects to be included in the pack file
                  HashSet::new(), // The set of objects that have repeated atleast once
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

/// Get the list of Git refs that need to be included in the packfile/bundle. On Mononoke end, this
/// will be bookmarks created from branches and tags. Branches and simple tags will be mapped to the
/// Git commit that they point to. Annotated tags will point to the Git objects that represent the tag
/// metadata
async fn refs_to_include(
    repo: &Repo,
    ctx: &CoreContext,
    bookmarks: &HashMap<Bookmark, ChangesetId>,
) -> Result<HashMap<String, ObjectId>> {
    stream::iter(bookmarks.iter())
        .map(|(bookmark, cs_id)| async move {
            if let BookmarkCategory::Tag = bookmark.key().category() {
                let tag_name = bookmark.key().name().to_string();
                let entry = repo
                    .bonsai_tag_mapping
                    .get_entry_by_tag_name(tag_name.clone())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in gettting bonsai_tag_mapping entry for tag name {}",
                            tag_name
                        )
                    })?;
                if let Some(entry) = entry {
                    let git_objectid = ObjectId::from_hex(entry.tag_hash.to_hex().as_bytes())
                        .with_context(|| {
                            format!(
                                "Error in converting GitSha1 {:?} to GitObjectId",
                                entry.tag_hash.to_hex()
                            )
                        })?;
                    let ref_name = format!("refs/{}", bookmark.key);
                    return anyhow::Ok((ref_name, git_objectid));
                }
            };
            let maybe_git_sha1 = repo
                .bonsai_git_mapping
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
            let ref_name = format!("refs/{}", bookmark.key);
            anyhow::Ok((ref_name, git_objectid))
        })
        .boxed()
        .buffer_unordered(100)
        .try_collect::<HashMap<_, _>>()
        .await
}

/// Generate a PackfileEntry for the given changeset and its corresponding GitDeltaManifestEntry
async fn packfile_entry(
    ctx: &CoreContext,
    repo: &Repo,
    changeset_id: ChangesetId,
    path: MPath,
    mut entry: GitDeltaManifestEntry,
    is_duplicated: bool,
) -> Result<PackfileItem> {
    let blobstore = repo.repo_blobstore_arc();
    // Can't use the delta if no delta variant is present in the entry. Additionally, if this object has been
    // duplicated across multiple commits in the pack, then we can't use it as a delta due to the potential of
    // a delta cycle
    let mut use_delta = entry.is_delta() && !is_duplicated;
    // Get the delta with the shortest size. Normally, we would also want to check if the base of the
    // delta is included in the pack (or at the client) but since this is a full clone, this check can be skipped
    entry.deltas.sort_by(|a, b| {
        a.instructions_compressed_size
            .cmp(&b.instructions_compressed_size)
    });
    let shortest_delta = entry.deltas.first();
    // Only use the delta if the size of the delta is less than 70% the size of the actual object
    use_delta &= shortest_delta.map_or(false, |delta| {
        (delta.instructions_uncompressed_size as f64) < (entry.full.size as f64) * 0.70
    });
    if use_delta {
        // Use the delta variant
        let delta = shortest_delta.unwrap(); // Should have a value by this point
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
    repo: &'a Repo,
    ctx: &'a CoreContext,
    changeset_id: ChangesetId,
    duplicated_objects: Arc<HashSet<ObjectId>>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let blobstore = repo.repo_blobstore_arc();
    let root_mf_id = repo
        .repo_derived_data
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
            let packfile_item = packfile_entry(ctx, repo, changeset_id, path, entry, is_duplicated).await?;
            yield packfile_item
        }
    };
    anyhow::Ok(Box::pin(objects_stream))
}

/// Create a stream of packfile items containing blob and tree objects that need to be included in the packfile/bundle.
/// In case the packfile item can be represented as a delta, then use the detla variant instead of the raw object
async fn blob_and_tree_packfile_stream<'a>(
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bookmarks: &'a HashMap<Bookmark, ChangesetId>,
    duplicated_objects: HashSet<ObjectId>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let target_commits = repo
        .commit_graph
        .ancestors_difference_stream(ctx, bookmarks.values().copied().collect(), vec![])
        .await
        .context("Error in getting ancestors difference")?;

    let duplicated_objects = Arc::new(duplicated_objects);
    // Get the packfile items corresponding to blob and tree objects in the repo. Where applicable, use delta to represent them
    // efficiently in the packfile/bundle
    let packfile_item_stream = target_commits
        .and_then(move |changeset_id| {
            blob_and_tree_packfile_items(repo, ctx, changeset_id, duplicated_objects.clone())
        })
        .try_flatten()
        .boxed();
    Ok(packfile_item_stream)
}

/// Create a stream of packfile items containing commit objects that need to be included in the packfile/bundle
async fn commit_packfile_stream<'a>(
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bookmarks: &'a HashMap<Bookmark, ChangesetId>,
) -> Result<BoxStream<'a, Result<PackfileItem>>> {
    let target_commits = repo
        .commit_graph
        .ancestors_difference_stream(ctx, bookmarks.values().copied().collect(), vec![])
        .await
        .context("Error in getting ancestors difference")?;
    let commit_stream = target_commits
        .and_then(move |changeset_id| async move {
            let blobstore = repo.repo_blobstore_arc();
            let maybe_git_sha1 = repo
                .bonsai_git_mapping
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
            let git_objectid =
                ObjectId::from_hex(git_sha1.to_hex().as_bytes()).with_context(|| {
                    format!(
                        "Error in converting GitSha1 {:?} to GitObjectId",
                        git_sha1.to_hex()
                    )
                })?;
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
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bookmarks: &'a HashMap<Bookmark, ChangesetId>,
) -> Result<(BoxStream<'a, Result<PackfileItem>>, usize)> {
    // Since we need the count of items, we would have to consume the stream either for counting or collecting the items.
    // This is fine, since unlike commits, blobs and trees there will only be thousands of tags in the worst case.
    let annotated_tags = stream::iter(bookmarks.keys())
        .filter_map(|bookmark| async move {
            // If the bookmark is actually a tag but there is no mapping in bonsai_tag_mapping table for it, then it
            // means that its a simple tag and won't be included in the packfile as an object. If a mapping exists, then
            // it will be included in the packfile as a raw Git object
            if let BookmarkCategory::Tag = bookmark.key().category() {
                let tag_name = bookmark.key().name().to_string();
                let entry = repo
                    .bonsai_tag_mapping
                    .get_entry_by_tag_name(tag_name.clone())
                    .await
                    .with_context(|| {
                        format!(
                            "Error in gettting bonsai_tag_mapping entry for tag name {}",
                            tag_name
                        )
                    })
                    .transpose();
                return entry;
            }
            None
        })
        .try_collect::<Vec<_>>()
        .await?;
    let tags_count = annotated_tags.len();
    let tag_stream = stream::iter(annotated_tags.into_iter().map(anyhow::Ok))
        .and_then(move |entry| async move {
            let blobstore = repo.repo_blobstore_arc();
            let git_objectid = ObjectId::from_hex(entry.tag_hash.to_hex().as_bytes())
                .with_context(|| {
                    format!(
                        "Error in converting GitSha1 {:?} to GitObjectId",
                        entry.tag_hash.to_hex()
                    )
                })?;
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

async fn create_from_mononoke_repo(
    ctx: &CoreContext,
    repo: Repo,
    output_file: tokio::fs::File,
) -> Result<()> {
    // We need to include all the bookmarks (i.e. branches, tags) in the bundle since its a full clone
    let bookmarks = all_bookmarks(&repo, ctx).await.with_context(|| {
        format!(
            "Error in fetching bookmarks for repo {}",
            repo.repo_identity.name()
        )
    })?;
    // STEP 1: Create state to track the total number of objects that will be included in the packfile/bundle. Initialize with the
    // tree, blob and commit count. Collect the set of duplicated objects.
    let (mut object_count, duplicated_objects) = object_count(&repo, ctx, &bookmarks).await?;

    // STEP 2: Create a mapping of all known bookmarks (i.e. branches, tags) and the commit that they point to. The commit should be represented
    // as a Git hash instead of a Bonsai hash since it will be part of the packfile/bundle
    let mut refs_to_include = refs_to_include(&repo, ctx, &bookmarks).await?;
    // Get the branch that the HEAD symref points to
    let head_ref = repo
        .git_symbolic_refs
        .get_ref_by_symref(HEAD_REF.to_string())
        .await
        .with_context(|| {
            format!(
                "Error in getting HEAD reference for repo {:?}",
                repo.repo_identity.name()
            )
        })?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "HEAD reference not found for repo {:?}",
                repo.repo_identity.name()
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

    // STEP 2.5: Adding the HEAD reference -> commit id mapping to refs_to_include map
    refs_to_include.insert(head_ref.symref_name.clone(), head_commit_id.clone());

    // STEP 3: Get the stream of blob and tree packfile items (with deltas where possible) to include in the pack/bundle. Note that
    // we have already counted these items as part of object count.
    let blob_and_tree_stream =
        blob_and_tree_packfile_stream(&repo, ctx, &bookmarks, duplicated_objects).await?;

    // STEP 4: Get the stream of commit packfile items to include in the pack/bundle. Note that we have already counted these items
    // as part of object count.
    let commit_stream = commit_packfile_stream(&repo, ctx, &bookmarks).await?;

    // STEP 5: Get the stream of tag packfile items to include in the pack/bundle. Note that we have not yet included the tag count in the
    // total object count so we will need the stream + count of elements in the stream
    let (tag_stream, tags_count) = tag_packfile_stream(&repo, ctx, &bookmarks).await?;
    // Include the tags in the object count since the tags will also be part of the packfile/bundle
    object_count += tags_count;
    // Generate the final packfile item stream by combining all the intermediate streams. The order of combination of these streams is not
    // important
    let packfile_stream = tag_stream.chain(commit_stream).chain(blob_and_tree_stream);

    // STEP 6: Write the generated packfile item stream to the bundle
    // Since this is a full clone
    let prereqs: Option<Vec<ObjectId>> = None;
    // Create the bundle writer with the header pre-written
    let mut writer = BundleWriter::new_with_header(
        output_file,
        refs_to_include.into_iter().collect(),
        prereqs,
        object_count as u32,
        DeltaForm::RefAndOffset, // Ref deltas are supported by Git when cloning from a bundle
    )
    .await?;

    writer
        .write(packfile_stream)
        .await
        .context("Error in writing packfile items to bundle")?;
    // Finish writing the bundle
    writer
        .finish()
        .await
        .context("Error in finishing write to bundle")?;
    Ok(())
}

async fn create_from_on_disk_repo(path: PathBuf, output_file: tokio::fs::File) -> Result<()> {
    // Create a handle for reading the Git directory
    let git_directory = std::fs::read_dir(path.as_path())
        .with_context(|| format!("Error in opening git directory {}", path.display()))?;
    let mut object_count = 0;
    let mut object_stream = None;
    let mut refs_to_include = HashMap::new();
    let mut head_ref = None;
    // Walk through the Git directory and fetch all objects and refs
    for entry in git_directory {
        let entry = entry.context("Error in opening entry within git directory")?;
        let is_entry_dir = entry.file_type()?.is_dir();
        let entry_name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Non UTF8 entry {:?} found in Git directory",
                    entry.file_name()
                )
            })?
            .to_string();
        match (is_entry_dir, entry_name.as_str()) {
            // Read all the entries within the objects directory and convert into the input stream
            // for writing to bundle
            (true, OBJECTS_DIR) => {
                let object_paths = get_files_in_dir_recursive(entry.path(), |entry| {
                    !NON_OBJECTS_DIR.contains(&entry.file_name().to_str().unwrap_or(""))
                })
                .context("Error in getting files from objects directory")?
                .into_iter()
                .filter(|entry| entry.is_file())
                .collect::<Vec<_>>();
                object_count += object_paths.len();
                object_stream = Some(get_objects_stream(object_paths).await);
            }
            (true, REFS_DIR) => {
                refs_to_include = get_refs(entry.path()).await?;
            }
            (false, HEAD_REF) => {
                head_ref = Some(get_head_ref(entry.path()).await?);
            }
            _ => {}
        };
    }
    // HEAD reference (if exists) points to some other ref instead of directly pointing to a commit.
    // To include the HEAD reference in the bundle, we need to find the commit it points to.
    if let Some(head_ref) = head_ref {
        let pointed_ref = refs_to_include.get(head_ref.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "HEAD ref points to a non-existent ref {}. Known refs: {:?}",
                head_ref,
                refs_to_include
            )
        })?;
        refs_to_include.insert("HEAD".to_string(), *pointed_ref);
    }
    // Create the bundle writer with the header pre-written
    let mut writer = BundleWriter::new_with_header(
        output_file,
        refs_to_include.into_iter().collect(),
        None,
        object_count as u32,
        DeltaForm::RefAndOffset,
    )
    .await?;
    let object_stream =
        object_stream.ok_or_else(|| anyhow::anyhow!("No objects found to write to bundle"))?;
    // Write the encoded Git object content to the Git bundle
    writer
        .write(object_stream)
        .await
        .context("Error in writing Git objects to bundle")?;
    // Finish writing the bundle
    writer
        .finish()
        .await
        .context("Error in finishing write to bundle")?;
    Ok(())
}

/// Get the ref pointed to by the HEAD reference in the given Git repo
async fn get_head_ref(head_path: PathBuf) -> Result<String> {
    let head_content = tokio::fs::read(head_path.as_path())
        .await
        .with_context(|| format!("Error in opening HEAD file {}", head_path.display()))?;
    let head_str = String::from_utf8(head_content)
        .with_context(|| format!("Non UTF8 content in HEAD file {}", head_path.display()))?;
    head_str
        .trim()
        .strip_prefix(HEAD_REF_PREFIX)
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("Invalid string content {} in HEAD file", head_str))
}

/// Get the list of refs from .git/refs directory along with the ObjectId that they point to.
async fn get_refs(refs_path: PathBuf) -> Result<HashMap<String, ObjectId>> {
    let ref_file_paths = get_files_in_dir_recursive(refs_path.clone(), |_| true)
        .with_context(|| {
            format!(
                "Error in fetching files from .git/refs directory {}",
                refs_path.display()
            )
        })?
        .into_iter()
        .filter(|entry| entry.is_file());
    stream::iter(ref_file_paths.filter(|path| path.is_file()).map(|path| {
        let refs_path = &refs_path;
        async move {
            // Read the contents of the ref file
            let ref_content = tokio::fs::read(path.as_path())
                .await
                .with_context(|| format!("Error in opening ref file {}", path.display()))?;
            // Parse the ref content into an Object ID
            let commit_id = ObjectId::from_hex(&ref_content[..40]).with_context(|| {
                format!(
                    "Error while parsing ref content {:?} at path {:} as Object ID",
                    ref_content.as_slice(),
                    path.display()
                )
            })?;
            let refs_header = path
                .strip_prefix(refs_path.parent().unwrap_or(refs_path.as_path()))
                .with_context(|| {
                    format!("Error in stripping prefix of ref path {}", path.display())
                })?;
            let refs_header = refs_header
                .to_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid non-UTF8 path {} in .git/refs",
                        refs_header.display()
                    )
                })?
                .to_string();
            anyhow::Ok((refs_header, commit_id))
        }
    }))
    .buffer_unordered(100)
    .try_collect::<HashMap<String, ObjectId>>()
    .await
}

/// Get the list of objects from the .git/objects directory and return
/// their content as a stream
async fn get_objects_stream(
    object_paths: Vec<PathBuf>,
) -> impl Stream<Item = Result<PackfileItem>> {
    stream::iter(object_paths.into_iter().map(anyhow::Ok)).and_then(move |path| {
        async move {
            // Fetch the Zlib encoded content of the Git object
            let encoded_data = tokio::fs::read(path.as_path())
                .await
                .with_context(|| format!("Error in opening objects file {}", path.display()))?;
            // Decode the content of the Git object
            let mut decoded_data = Vec::new();
            let mut decoder = ZlibDecoder::new(decoded_data);
            decoder.write_all(encoded_data.as_ref())?;
            decoded_data = decoder.finish()?;
            PackfileItem::new_base(Bytes::from(decoded_data))
        }
    })
}

fn get_files_in_dir_recursive<P>(path: PathBuf, predicate: P) -> Result<Vec<PathBuf>>
where
    P: FnMut(&walkdir::DirEntry) -> bool,
{
    let file_paths = WalkDir::new(path)
        .into_iter()
        .filter_entry(predicate)
        .map(|result| result.map(|entry| entry.into_path()))
        .collect::<std::result::Result<Vec<_>, walkdir::Error>>()?;
    Ok(file_paths)
}
