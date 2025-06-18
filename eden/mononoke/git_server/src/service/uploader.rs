/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
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
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::hash::GitSha1;
use repo_identity::RepoIdentityRef;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::log_git_content_ref;
use slog::info;
use topo_sort::sort_topological;

use super::reader::GitObjectStore;
use crate::Repo;
use crate::command::RefUpdate;

type ContentTags = HashMap<String, ObjectId>;

#[derive(Clone, Debug)]
struct TagMetadata {
    name: String,
    bonsai_target: Option<ChangesetId>,
    git_target: ObjectId,
}

impl TagMetadata {
    fn new(name: String, bonsai_target: Option<ChangesetId>, git_target: ObjectId) -> Self {
        Self {
            name,
            bonsai_target,
            git_target,
        }
    }
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
            let tag_name_from_object = object
                .with_parsed_as_tag(|tag| tag.name.to_string())
                .ok_or_else(|| anyhow::anyhow!("Expected {} to be a tag object", id.to_hex()))?;
            result.insert(
                id.clone(),
                TagMetadata::new(tag_name_from_object, bonsai_id, target_id),
            );
        }
    }
    Ok(result)
}

/// Method responsible for processing all the tags that are part of this push and returning the
/// content tags among them
async fn process_tags<Uploader: GitUploader>(
    ctx: &CoreContext,
    repo: &Repo,
    uploader: Arc<Uploader>,
    object_store: Arc<GitObjectStore>,
    ref_map: &mut RefMap,
    ref_updates: &[RefUpdate],
) -> Result<ContentTags> {
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
            // would atleast end with the tag object name in case of namespaced tags
            if ref_name.ends_with(tag_metadata.name.as_str()) {
                tag_metadata.name = ref_name;
            } else {
                // Otherwise, remove the tag entry from the map
                tags.remove(oid);
            }
        }
    }
    info!(ctx.logger(), "Uploading tags for repo {}", repo_name);
    // Upload the tags to the blobstore and also create bonsai mapping for it
    for (tag_id, tag_metadata) in tags {
        let TagMetadata {
            name,
            bonsai_target,
            git_target,
        } = tag_metadata;
        // Add a mapping from the tag object id to the commit changeset id where it points. This will later
        // be used in bookmark movement
        if let Some(bonsai_target) = bonsai_target.as_ref() {
            ref_map.insert_tag(&tag_id, *bonsai_target);
        } else {
            content_tags.insert(format!("refs/{}", name), git_target);
        }
        // Store the raw tag object first
        upload_git_tag(ctx, uploader.clone(), object_store.clone(), &tag_id).await?;
        // Create the changeset corresponding to the commit pointed to by the tag.
        create_changeset_for_annotated_tag(
            ctx,
            uploader.clone(),
            object_store.clone(),
            &tag_id,
            Some(name),
            bonsai_target,
        )
        .await?;
    }
    Ok(content_tags)
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
) -> Result<(RefMap, Vec<RefUpdate>)> {
    let repo_name = repo.repo_identity().name().to_string();
    let uploader = Arc::new(DirectUploader::with_arc(
        repo.clone(),
        ReuploadCommits::Never,
    ));
    let prefs = GitimportPreferences {
        backfill_derivation: BackfillDerivation::OnlySpecificTypes(vec![
            DerivableType::GitDeltaManifestsV2,
        ]),
        concurrency,
        lfs,
        ..Default::default()
    };
    let acc = GitimportAccumulator::from_roots(HashMap::new());
    info!(
        ctx.logger(),
        "Importing commit contexts for repo {}", repo_name
    );
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
    let content_tags = process_tags(
        ctx,
        &repo,
        uploader.clone(),
        object_store.clone(),
        &mut ref_map,
        ref_updates,
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
    Ok((ref_map, ref_updates))
}
