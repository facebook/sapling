/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Rewrite a linear stack of draft commits: apply a per-commit edit and
//! re-parent each commit onto the rewritten form of its parent.

use std::collections::HashMap;

use anyhow::Context as _;
use anyhow::Result;
use blobstore::Loadable;
use changesets_creation::save_changesets;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;

/// Same keys pushrebase strips: they describe the original commit's history.
const MUTATION_KEYS: &[&str] = &["mutpred", "mutuser", "mutdate", "mutop", "mutsplit"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewrittenCommit {
    pub original: ChangesetId,
    pub rewritten: ChangesetId,
}

/// Draft commits from `tip` down while `include` holds, bottom first.
/// Stops at a merge, a root or a public commit, checked before each load.
pub async fn load_draft_stack(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + PhasesRef),
    tip: ChangesetId,
    include: impl Fn(ChangesetId) -> bool,
) -> Result<Vec<BonsaiChangeset>> {
    let mut chain: Vec<BonsaiChangeset> = Vec::new();
    let mut cursor = Some(tip);
    while let Some(cs_id) = cursor.filter(|cs_id| include(*cs_id)) {
        let public = repo.phases().get_public(ctx, vec![cs_id], false).await?;
        if public.contains(&cs_id) {
            break;
        }
        let bcs = cs_id
            .load(ctx, repo.repo_blobstore())
            .await
            .with_context(|| format!("loading changeset {cs_id}"))?;
        cursor = {
            let mut parents = bcs.parents();
            match (parents.next(), parents.next()) {
                (Some(parent), None) => Some(parent),
                _ => None,
            }
        };
        chain.push(bcs);
    }
    chain.reverse();
    Ok(chain)
}

/// Rewrites `stack` (bottom first) with `edit` applied to each commit and
/// saves the results; unchanged commits are skipped. Deterministic.
pub async fn rewrite_stack(
    ctx: &CoreContext,
    repo: &(impl CommitGraphRef + CommitGraphWriterRef + RepoBlobstoreRef + RepoIdentityRef),
    stack: Vec<BonsaiChangeset>,
    mut edit: impl FnMut(ChangesetId, &mut BonsaiChangesetMut) -> Result<()>,
) -> Result<Vec<RewrittenCommit>> {
    let mut remap: HashMap<ChangesetId, ChangesetId> = HashMap::new();
    let mut rewritten = Vec::with_capacity(stack.len());
    let mut bonsais = Vec::with_capacity(stack.len());
    for bcs in stack {
        let original = bcs.get_changeset_id();
        let mut bcs = bcs.into_mut();
        bcs.parents = bcs
            .parents
            .into_iter()
            .map(|parent| remap.get(&parent).copied().unwrap_or(parent))
            .collect();
        for file_change in bcs.file_changes.values_mut() {
            if let FileChange::Change(tc) = file_change {
                let copy_from = tc
                    .copy_from()
                    .map(|(path, cs)| (path.clone(), remap.get(cs).copied().unwrap_or(*cs)));
                *file_change = FileChange::Change(tc.with_new_copy_from(copy_from));
            }
        }
        for (_path, change) in bcs.subtree_changes.iter_mut() {
            if let Some((from, _)) = change.change_source() {
                if let Some(new_from) = remap.get(&from) {
                    change.replace_source_changeset_id(*new_from);
                }
            }
        }
        for key in MUTATION_KEYS {
            bcs.hg_extra.remove(*key);
        }
        edit(original, &mut bcs)?;
        let bcs = bcs.freeze()?;
        let new_id = bcs.get_changeset_id();
        if new_id == original {
            continue;
        }
        remap.insert(original, new_id);
        rewritten.push(RewrittenCommit {
            original,
            rewritten: new_id,
        });
        bonsais.push(bcs);
    }
    if !bonsais.is_empty() {
        save_changesets(ctx, repo, bonsais).await?;
    }
    Ok(rewritten)
}
