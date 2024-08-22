/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaisOrGitShas;
use bookmarks::BookmarkKey;
use context::CoreContext;
use gix_hash::ObjectId;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use crate::bookmarks_provider::bookmarks;
use crate::bookmarks_provider::list_tags;
use crate::types::CommitTagMappings;
use crate::types::RefTarget;
use crate::types::RefsSource;
use crate::types::RequestedRefs;
use crate::types::RequestedSymrefs;
use crate::types::TagInclusion;
use crate::types::TranslatedShas;
use crate::utils::symref_target;
use crate::Repo;
use crate::HEAD_REF;
use crate::REF_PREFIX;

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
    // Fetch the bookmarks from the WBC since this is Git read path and we are fine with some staleness
    let wanted_refs = bookmarks(ctx, repo, &requested_refs, RefsSource::WarmBookmarksCache)
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

/// Fetch the corresponding bonsai commits for the input Git object ids. If the object id doesn't
/// correspond to a bonsai commit, try to resolve it to a tag and then fetch the bonsai commit and
/// return it along with the tag name
pub(crate) async fn git_shas_to_bonsais(
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

/// Fetch all the bonsai commits pointed to by the annotated tags corresponding
/// to the input object ids along with the tag names. For all the input Git shas
/// that we could not find a corresponding tag for, return the shas as blob and tree
/// objects
pub(crate) async fn tagged_commits(
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
    // Use WBC for fetching bookmarks since this is Git read path
    let tagged_commits = list_tags(ctx, repo, RefsSource::WarmBookmarksCache)
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

/// Get the list of Git refs that need to be included in the stream of PackfileItem. On Mononoke end, this
/// will be bookmarks created from branches and tags. Branches and simple tags will be mapped to the
/// Git commit that they point to. Annotated tags will be handled based on the `tag_inclusion` parameter
pub(crate) async fn refs_to_include(
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

/// The HEAD ref in Git doesn't have a direct counterpart in Mononoke bookmarks and is instead
/// stored in the git_symbolic_refs. Fetch the mapping and add them to the list of refs to include
pub(crate) async fn include_symrefs(
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
            if let Some(head_commit_id) = refs_to_include
                .get(&head_ref.ref_name_with_type())
                .map(|target| target.id())
            {
                let ref_target = symref_target(
                    &head_ref.ref_name_with_type(),
                    head_commit_id.clone(),
                    symref_format,
                );
                FxHashMap::from_iter([(head_ref.symref_name, ref_target)])
            } else {
                // Silently ignore the Symrefs that point to a non-existent
                // Git branch to maintain parity with Git
                FxHashMap::default()
            }
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
            symref_entries
                .into_iter()
                .filter_map(|entry| {
                    // Silently ignore the Symrefs that point to a non-existent
                    // Git branch to maintain parity with Git
                    refs_to_include
                        .get(&entry.ref_name_with_type())
                        .map(|target| (entry, target.id()))
                })
                .map(|(entry, ref_commit_id)| {
                    let ref_target = symref_target(
                        &entry.ref_name_with_type(),
                        ref_commit_id.clone(),
                        symref_format,
                    );
                    Ok((entry.symref_name, ref_target))
                })
                .collect::<Result<FxHashMap<_, _>>>()?
        }
        RequestedSymrefs::ExcludeAll => FxHashMap::default(),
    };

    // Add the symref -> commit mapping to the refs_to_include map
    refs_to_include.extend(symref_commit_mapping.into_iter());
    Ok(())
}
