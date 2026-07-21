/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use cloned::cloned;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use gix_hash::ObjectId;
use gix_object::Kind;
use import_direct::DirectUploader;
use import_tools::BackfillDerivation;
use import_tools::GitImportLfs;
use import_tools::GitUploader;
use import_tools::GitimportAccumulator;
use import_tools::GitimportPreferences;
use import_tools::ReuploadCommits;
use import_tools::create_changeset_for_annotated_tag;
use import_tools::git_reader::GitReader;
use import_tools::import_commit_contents;
use import_tools::upload_git_object;
use import_tools::upload_git_tag;
use import_tools::upload_git_tree_recursively;
use mononoke_api::repo::git::TagMappingWrite;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::hash::GitSha1;
use repo_identity::RepoIdentityRef;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::log_git_content_ref;
use topo_sort::sort_topological;
use tracing::info;

use super::reader::GitObjectStore;
use crate::Repo;
use crate::command::RefUpdate;

type ContentTags = HashMap<String, ObjectId>;

#[derive(Clone, Debug)]
struct TagMetadata {
    name: String,
    bonsai_target: Option<ChangesetId>,
    git_target: ObjectId,
    target_is_tag: bool,
}

/// Content tags plus the annotated-tag mappings deferred to the bookmark-move
/// phase (empty unless atomic mode is on).
struct ProcessedTags {
    content_tags: ContentTags,
    tag_mapping_entries: Vec<BonsaiTagMappingEntry>,
}

/// Everything an `upload_objects` call produces for a push.
pub struct UploadedObjects {
    pub ref_map: RefMap,
    pub ref_updates: Vec<RefUpdate>,
    /// Annotated-tag mappings to write atomically during the bookmark move.
    pub tag_mapping_entries: Vec<BonsaiTagMappingEntry>,
}

#[derive(Clone, Debug)]
pub struct RefMap {
    commits_to_bonsai: HashMap<ObjectId, ChangesetId>,
    bonsai_to_commits: HashMap<ChangesetId, ObjectId>,
    tags_to_bonsai: HashMap<ObjectId, ChangesetId>,
    bonsai_to_tags: HashMap<ChangesetId, ObjectId>,
}

impl RefMap {
    fn from_commits(commits: HashMap<ObjectId, ChangesetId>) -> Self {
        let bonsai_to_commits = commits.iter().map(|(oid, cs_id)| (*cs_id, *oid)).collect();
        Self {
            bonsai_to_commits,
            commits_to_bonsai: commits,
            tags_to_bonsai: HashMap::new(),
            bonsai_to_tags: HashMap::new(),
        }
    }

    fn commit_bonsai_by_oid(&self, oid: &ObjectId) -> Option<ChangesetId> {
        self.commits_to_bonsai.get(oid).cloned()
    }

    pub(crate) fn bonsai_by_oid(&self, oid: &ObjectId) -> Option<ChangesetId> {
        self.commits_to_bonsai
            .get(oid)
            .cloned()
            .or_else(|| self.tags_to_bonsai.get(oid).cloned())
    }

    pub(crate) fn oid_by_bonsai(&self, cs_id: &ChangesetId) -> Option<ObjectId> {
        self.bonsai_to_commits
            .get(cs_id)
            .cloned()
            .or_else(|| self.bonsai_to_tags.get(cs_id).cloned())
    }

    fn insert_tag(&mut self, oid: &ObjectId, cs_id: ChangesetId) {
        self.tags_to_bonsai.insert(*oid, cs_id);
        self.bonsai_to_tags.insert(cs_id, *oid);
    }
}

/// Method responsible for fetching all commits from the object store and
/// topologically sorting them.
fn sorted_commits(object_store: &GitObjectStore) -> Result<Vec<ObjectId>> {
    let commits_with_parents = object_store
        .object_map
        .iter()
        .filter_map(|(oid, object)| {
            object.with_parsed_as_commit(|commit| {
                Ok::<_, anyhow::Error>((
                    *oid,
                    commit
                        .parents
                        .iter()
                        .map(|id| ObjectId::from_hex(id))
                        .collect::<Result<Vec<ObjectId>, _>>()?,
                ))
            })
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    sort_topological(&commits_with_parents).ok_or_else(|| {
        anyhow::anyhow!("Unable to sort git commits from packfile due to the existence of a cycle")
    })
}

async fn git_to_bonsai(
    ctx: &CoreContext,
    repo: &Repo,
    commit_id: &ObjectId,
) -> Result<ChangesetId> {
    let sha = GitSha1::from_object_id(commit_id.as_ref())
        .context("Error while converting Git Object Id to Git Sha1 during push")?;
    // Get the bonsai commits corresponding to the Git shas
    repo.bonsai_git_mapping()
        .get(ctx, BonsaisOrGitShas::GitSha1(vec![sha]))
        .await
        .with_context(|| {
            format!(
                "Failed to fetch bonsai_git_mapping for repo {}",
                repo.repo_identity().name()
            )
        })?
        .first()
        .map(|entry| entry.bcs_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Bonsai commit does not exist for git commit {}",
                commit_id.to_hex()
            )
        })
}

/// Method responsible for fetching all tags from the object store
async fn tags(
    ctx: &CoreContext,
    repo: &Repo,
    object_store: &GitObjectStore,
    ref_map: &RefMap,
) -> Result<HashMap<ObjectId, TagMetadata>> {
    let mut result = HashMap::new();
    for (id, object) in object_store.object_map.iter() {
        if object.is_tag()
            && let Ok((kind, target_id)) = object_store.peel_to_target(*id).await
        {
            let bonsai_id = if let Some(bonsai_id) = ref_map.commit_bonsai_by_oid(&target_id) {
                Some(bonsai_id)
            } else if kind != Kind::Commit {
                // If the target is not a commit, we can't create a changeset for it
                None
            } else {
                Some(git_to_bonsai(ctx, repo, &target_id).await?)
            };
            let (tag_name_from_object, target_is_tag) = object
                .with_parsed_as_tag(|tag| (tag.name.to_string(), tag.target_kind == Kind::Tag))
                .ok_or_else(|| anyhow::anyhow!("Expected {} to be a tag object", id.to_hex()))?;
            result.insert(
                id.clone(),
                TagMetadata {
                    name: tag_name_from_object,
                    bonsai_target: bonsai_id,
                    git_target: target_id,
                    target_is_tag,
                },
            );
        }
    }
    Ok(result)
}

/// Upload one tag's object + changeset. Returns the mapping entry when the write
/// is deferred to the bookmark-move phase (atomic mode), otherwise `None`.
async fn upload_tag<Uploader: GitUploader>(
    ctx: &CoreContext,
    uploader: Arc<Uploader>,
    object_store: Arc<GitObjectStore>,
    tag_id: ObjectId,
    name: String,
    bonsai_target: Option<ChangesetId>,
    target_is_tag: bool,
    mapping_write: TagMappingWrite,
) -> Result<Option<BonsaiTagMappingEntry>> {
    upload_git_tag(ctx, uploader.clone(), object_store.clone(), &tag_id).await?;
    let changeset_id = create_changeset_for_annotated_tag(
        ctx,
        uploader,
        object_store,
        &tag_id,
        Some(name.clone()),
        bonsai_target,
        mapping_write,
    )
    .await?;
    Ok(match mapping_write {
        TagMappingWrite::Inline => None,
        TagMappingWrite::Deferred => Some(BonsaiTagMappingEntry {
            changeset_id,
            tag_name: name,
            tag_hash: GitSha1::from_bytes(tag_id.as_bytes())?,
            target_is_tag,
        }),
    })
}

/// Process all tags in a push: upload each tag object + changeset, populate
/// `ref_map` / content tags, and return any annotated-tag mappings deferred to
/// the bookmark-move phase.
async fn process_tags<Uploader: GitUploader>(
    ctx: &CoreContext,
    repo: &Repo,
    uploader: Arc<Uploader>,
    object_store: Arc<GitObjectStore>,
    ref_map: &mut RefMap,
    ref_updates: &[RefUpdate],
    // Whether annotated-tag mappings are deferred to the bookmark-move phase.
    atomic_tag_mapping: bool,
) -> Result<ProcessedTags> {
    let repo_name = repo.repo_identity().name().to_string();
    let mut content_tags = HashMap::new();
    // Fetch all the tags to be uploaded as part of this push
    let mut tags = tags(ctx, repo, &object_store, ref_map).await?;
    // Ensure that the tags are mapped to the right name (necessary for tags with namespaced refs)
    for ref_update in ref_updates {
        let (name, oid) = (ref_update.ref_name.clone(), ref_update.to.as_ref());
        let ref_name = name
            .strip_prefix("refs/")
            .map_or(name.to_string(), |name| name.to_string());
        if let Some(tag_metadata) = tags.get_mut(oid) {
            // Only update the tag name based on the ref name if we are sure they refer to the same tag
            // If they refer to the same tag, the names would either be identical or the ref name would
            // would at least end with the tag object name in case of namespaced tags
            if ref_name.ends_with(tag_metadata.name.as_str()) {
                tag_metadata.name = ref_name;
            } else {
                // Otherwise, remove the tag entry from the map
                tags.remove(oid);
            }
        }
    }
    info!("Uploading tags for repo {}", repo_name);
    // Only annotated tags directly pushed as a ref get a name-matched bookmark
    // move (whose transaction writes the mapping); inner tags of a nested chain
    // are in the pack but unreferenced, so they keep writing the mapping inline.
    let referenced_oids: HashSet<ObjectId> =
        ref_updates.iter().map(|ref_update| ref_update.to).collect();
    let upload_items: Vec<_> = tags
        .into_iter()
        .map(|(tag_id, tag_metadata)| {
            let TagMetadata {
                name,
                bonsai_target,
                git_target,
                target_is_tag,
            } = tag_metadata;
            if let Some(bonsai_target) = bonsai_target.as_ref() {
                ref_map.insert_tag(&tag_id, *bonsai_target);
            } else {
                content_tags.insert(format!("refs/{name}"), git_target);
            }
            let mapping_write = if atomic_tag_mapping
                && bonsai_target.is_some()
                && referenced_oids.contains(&tag_id)
            {
                TagMappingWrite::Deferred
            } else {
                TagMappingWrite::Inline
            };
            (tag_id, name, bonsai_target, target_is_tag, mapping_write)
        })
        .collect();
    let tag_mapping_entries = stream::iter(upload_items)
        .map(
            |(tag_id, name, bonsai_target, target_is_tag, mapping_write)| {
                cloned!(uploader, object_store);
                upload_tag(
                    ctx,
                    uploader,
                    object_store,
                    tag_id,
                    name,
                    bonsai_target,
                    target_is_tag,
                    mapping_write,
                )
            },
        )
        .buffer_unordered(20)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    Ok(ProcessedTags {
        content_tags,
        tag_mapping_entries,
    })
}

/// Method responsible for uploading git tree and blob objects pointed to by the content refs
/// that are part of this push
async fn upload_content_ref_objects<Uploader: GitUploader, Reader: GitReader>(
    ctx: &CoreContext,
    repo: Arc<Repo>,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    ref_updates: &[RefUpdate],
    content_tags: ContentTags,
) -> Result<Vec<RefUpdate>> {
    stream::iter(ref_updates.to_vec())
        .map(anyhow::Ok)
        .map_ok(|ref_update| {
            cloned!(uploader, reader, ctx, content_tags, repo);
            async move {
                let repo_name = repo.repo_identity().name().to_string();
                let (ref_name, mut git_hash) = (ref_update.ref_name.clone(), ref_update.to.clone());
                let delete_ref = git_hash.is_null();
                // If the ref is getting deleted, use the old value of the ref
                if delete_ref {
                    git_hash = ref_update.from.clone();
                }
                // If the ref is a tag, use the content_tags mapping to get the git hash of the peeled object
                if let Some(tag_id) = content_tags.get(&ref_name) {
                    git_hash = tag_id.clone();
                }
                let obj_kind = reader
                    .get_object(git_hash.as_ref())
                    .await?
                    .with_parsed(|parsed| parsed.kind());
                let is_content_ref = obj_kind.is_tree() || obj_kind.is_blob();
                // If the ref is a content ref, then upload the objects pointed at by the ref. Nothing needs to
                // be uploaded if the ref is getting deleted.
                if !delete_ref && is_content_ref {
                    if obj_kind.is_tree() {
                        // The object pointed at is a tree. Ensure that all the members of the tree are uploaded
                        // recursively
                        upload_git_tree_recursively(
                            &ctx,
                            uploader.clone(),
                            reader.clone(),
                            &git_hash,
                        )
                        .await?;
                    } else {
                        upload_git_object(&ctx, uploader.clone(), reader.clone(), &git_hash)
                            .await?;
                    }
                    let ref_for_content_mapping = ref_name
                        .strip_prefix("refs/")
                        .unwrap_or(&ref_name)
                        .to_string();
                    let content_ref_info = GitContentRefInfo {
                        repo_name,
                        ref_name: ref_for_content_mapping.clone(),
                        git_hash: git_hash.to_hex().to_string(),
                        object_type: obj_kind.to_string(),
                    };
                    uploader
                        .generate_ref_content_mapping(
                            &ctx,
                            ref_for_content_mapping,
                            git_hash,
                            obj_kind.is_tree(),
                        )
                        .await?;
                    log_git_content_ref(&ctx, &repo, &content_ref_info).await;
                }

                anyhow::Ok(RefUpdate::new(
                    ref_name,
                    obj_kind.into(),
                    ref_update.from,
                    ref_update.to,
                ))
            }
        })
        .try_buffer_unordered(20) // Content refs should be very rare
        .try_collect::<Vec<_>>()
        .await
}

/// Method responsible for uploading git and bonsai objects corresponding to the objects
/// present in the input object_store
pub async fn upload_objects(
    ctx: &CoreContext,
    repo: Arc<Repo>,
    object_store: Arc<GitObjectStore>,
    ref_updates: &[RefUpdate],
    lfs: GitImportLfs,
    concurrency: usize,
    persist_partial_mappings: bool,
    atomic_tag_mapping: bool,
) -> Result<UploadedObjects> {
    let repo_name = repo.repo_identity().name().to_string();
    let uploader = Arc::new(DirectUploader::with_arc(
        repo.clone(),
        ReuploadCommits::Never,
    ));
    let prefs = GitimportPreferences {
        backfill_derivation: BackfillDerivation::OnlySpecificTypes(vec![
            DerivableType::GitDeltaManifestsV2,
            DerivableType::GitDeltaManifestsV3,
        ]),
        concurrency,
        lfs,
        persist_partial_mappings,
        ..Default::default()
    };
    let acc = GitimportAccumulator::from_roots(HashMap::new());
    info!("Importing commit contexts for repo {}", repo_name);
    // Import and store all the commits, trees and blobs that are pare of the push
    let git_bonsai_mappings = import_commit_contents(
        ctx,
        repo_name.clone(),
        sorted_commits(&object_store)?,
        uploader.clone(),
        object_store.clone(),
        &prefs,
        acc,
    )
    .await?
    .into_iter()
    .collect();
    let mut ref_map = RefMap::from_commits(git_bonsai_mappings);

    // Process all the tags that are part of this push
    let ProcessedTags {
        content_tags,
        tag_mapping_entries,
    } = process_tags(
        ctx,
        &repo,
        uploader.clone(),
        object_store.clone(),
        &mut ref_map,
        ref_updates,
        atomic_tag_mapping,
    )
    .await
    .context("Error during process_tags")?;
    // Upload all the content refs that are part of this push
    let ref_updates = upload_content_ref_objects(
        ctx,
        repo.clone(),
        uploader,
        object_store,
        ref_updates,
        content_tags,
    )
    .await
    .context("Error during upload_content_ref_objects")?;
    Ok(UploadedObjects {
        ref_map,
        ref_updates,
        tag_mapping_entries,
    })
}
