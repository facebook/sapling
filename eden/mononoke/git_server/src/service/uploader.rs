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
use async_recursion::async_recursion;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
use context::CoreContext;
use gix_hash::ObjectId;
use gix_object::Kind;
use gix_object::Tag;
use import_direct::DirectUploader;
use import_tools::create_changeset_for_annotated_tag;
use import_tools::git_reader::GitReader;
use import_tools::import_commit_contents;
use import_tools::upload_git_tag;
use import_tools::BackfillDerivation;
use import_tools::GitImportLfs;
use import_tools::GitimportAccumulator;
use import_tools::GitimportPreferences;
use import_tools::ReuploadCommits;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use repo_identity::RepoIdentityRef;
use topo_sort::sort_topological;

use super::reader::GitObjectStore;
use crate::command::RefUpdate;
use crate::Repo;

#[derive(Clone, Debug)]
struct TagMetadata {
    name: Option<String>,
    bonsai_target: ChangesetId,
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

impl TagMetadata {
    fn new(name: Option<String>, bonsai_target: ChangesetId) -> Self {
        Self {
            name,
            bonsai_target,
        }
    }
}

/// Method responsible for fetching all commits from the object store and
/// topologically sorting them.
fn sorted_commits(object_store: &GitObjectStore) -> Result<Vec<ObjectId>> {
    let commits_with_parents = object_store
        .object_map
        .iter()
        .filter_map(|(oid, object)| {
            object
                .parsed
                .as_commit()
                .map(|commit| (oid.clone(), commit.parents.clone().to_vec()))
        })
        .collect::<HashMap<_, _>>();
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

#[async_recursion]
pub(crate) async fn peel_tag_target(
    tag: &Tag,
    object_store: &GitObjectStore,
) -> Result<(ObjectId, Kind)> {
    // If the target is a tag, keep recursing
    if tag.target_kind == Kind::Tag {
        let target_tag = object_store.read_tag(&tag.target).await?;
        peel_tag_target(&target_tag, object_store).await
    } else {
        Ok((tag.target.clone(), tag.target_kind))
    }
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
        if let Some(tag) = object.parsed.as_tag() {
            let (commit_id, kind) = peel_tag_target(tag, object_store).await?;
            // If the tag points to a tree or a blob, then we don't care for it cause it will get rejected
            // later in the push
            if kind != Kind::Commit {
                continue;
            }
            let bonsai_id = if let Some(bonsai_id) = ref_map.commit_bonsai_by_oid(&commit_id) {
                bonsai_id
            } else {
                git_to_bonsai(ctx, repo, &commit_id).await?
            };
            result.insert(id.clone(), TagMetadata::new(None, bonsai_id));
        }
    }
    Ok(result)
}

/// Method responsible for uploading git and bonsai objects corresponding to the objects
/// present in the input object_store
pub async fn upload_objects(
    ctx: &CoreContext,
    repo: Arc<Repo>,
    object_store: Arc<GitObjectStore>,
    ref_updates: &[RefUpdate],
    lfs: GitImportLfs,
) -> Result<RefMap> {
    let repo_name = repo.repo_identity().name().to_string();
    let uploader = Arc::new(DirectUploader::with_arc(
        repo.clone(),
        ReuploadCommits::Never,
    ));
    let prefs = GitimportPreferences {
        backfill_derivation: BackfillDerivation::OnlySpecificTypes(vec![
            DerivableType::GitDeltaManifestsV2,
        ]),
        concurrency: 100,
        lfs,
        ..Default::default()
    };
    let acc = GitimportAccumulator::from_roots(HashMap::new());
    // Import and store all the commits, trees and blobs that are pare of the push
    let git_bonsai_mappings = import_commit_contents(
        ctx,
        repo_name,
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

    // Fetch all the tags to be uploaded as part of this push
    let mut tags = tags(ctx, &repo, &object_store, &ref_map).await?;
    // Ensure that the tags are mapped to the right name (necessary for tags with namespaced refs)
    for ref_update in ref_updates {
        let (name, oid) = (ref_update.ref_name.clone(), ref_update.to.as_ref());
        let ref_name = name
            .strip_prefix("refs/")
            .map_or(name.to_string(), |name| name.to_string());
        tags.entry(oid.to_owned()).and_modify(|tag_metadata| {
            tag_metadata.name = Some(ref_name);
        });
    }
    // Upload the tags to the blobstore and also create bonsai mapping for it
    for (tag_id, tag_metadata) in tags {
        let TagMetadata {
            name,
            bonsai_target,
        } = tag_metadata;
        // Add a mapping from the tag object id to the commit changeset id where it points. This will later
        // be used in bookmark movement
        ref_map.insert_tag(&tag_id, bonsai_target);
        // Store the raw tag object first
        upload_git_tag(ctx, uploader.clone(), object_store.clone(), &tag_id).await?;
        // Create the changeset corresponding to the commit pointed to by the tag.
        create_changeset_for_annotated_tag(
            ctx,
            uploader.clone(),
            object_store.clone(),
            &tag_id,
            name,
            &bonsai_target,
        )
        .await?;
    }
    Ok(ref_map)
}
