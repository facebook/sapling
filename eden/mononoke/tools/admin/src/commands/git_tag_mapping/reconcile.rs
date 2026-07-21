/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bonsai_tag_mapping::Freshness;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks_movement::BookmarkKind;
use bookmarks_movement::check_bookmark_sync_config;
use clap::Args;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTagTarget;
use mononoke_types::hash::Blake2;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstoreRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;
use repo_update_logger::log_bookmark_operation;

use super::Repo;

const CONCURRENCY: usize = 50;

#[derive(Args)]
pub struct ReconcileArgs {
    /// Recover the diverged tags by moving each tags/<tag> bookmark back to its
    /// annotated tag's target. Without this flag only reports them (dry-run).
    #[clap(long)]
    apply: bool,

    /// Restrict to these tag names (repeatable or comma-separated), matched
    /// against the full bonsai_tag_mapping name shown in the dry-run output
    /// (e.g. `--tag tags/3.4.14475`). With --apply, exactly one of --tag or
    /// --all is required: a divergence can also be a legitimate annotated->
    /// lightweight conversion, which recovery would wrongly revert.
    #[clap(long, value_delimiter = ',')]
    tag: Vec<String>,

    /// With --apply, act on every diverged tag instead of an explicit --tag
    /// list. Opt-in, because it reverts any genuine annotated->lightweight
    /// conversions in the diverged set.
    #[clap(long, conflicts_with = "tag")]
    all: bool,

    /// Bypass the source-of-truth / push-redirect safety guards. Only use when
    /// certain Mononoke owns the repo and it is not megarepo-synced.
    #[clap(long)]
    force: bool,
}

/// A `tags/<tag>` bookmark that has diverged from its annotated tag object: the
/// bookmark points at `bookmark`, but the annotated tag object's own target is
/// `mapping_target`. Recovery moves the bookmark to `mapping_target` so it
/// matches the advertised tag object, keeping the tag annotated (S687348) rather
/// than downgrading it to a lightweight tag.
struct Divergence {
    tag_name: String,
    bookmark_key: BookmarkKey,
    /// The commit the annotated tag object points at (the correct target).
    mapping_target: ChangesetId,
    /// The commit the bookmark currently (wrongly) points at.
    bookmark: ChangesetId,
}

/// The all-zeros placeholder that `create_annotated_tag` stores as the target of
/// content (tree/blob) or otherwise-unresolved tags. Such tags have no commit to
/// move a bookmark to, so they must never be treated as diverged.
fn placeholder_target() -> ChangesetId {
    ChangesetId::new(Blake2::from_byte_array([0; 32]))
}

/// A mapping is diverged only when the `tags/<tag>` bookmark EXISTS and points at
/// a different changeset than the annotated tag object's (real) target.
/// Deliberately NOT diverged (so never touched): a missing bookmark (orphan /
/// content tag), a content/unresolved target (all-zeros placeholder), or
/// metadata that is not an annotated tag.
async fn divergence_for(
    repo: &Repo,
    ctx: &CoreContext,
    entry: BonsaiTagMappingEntry,
) -> Result<Option<Divergence>> {
    let bookmark_key = BookmarkKey::new(&entry.tag_name)
        .with_context(|| format!("Invalid tag bookmark name {}", entry.tag_name))?;
    let bookmark = repo
        .bookmarks()
        .get(ctx.clone(), &bookmark_key, bookmarks::Freshness::MostRecent)
        .await
        .with_context(|| format!("Failed to resolve bookmark {bookmark_key}"))?;
    let metadata = entry
        .changeset_id
        .load(ctx, repo.repo_blobstore())
        .await
        .with_context(|| {
            format!(
                "Failed to load metadata changeset for tag {}",
                entry.tag_name
            )
        })?;
    let mapping_target = match metadata.git_annotated_tag() {
        Some(tag) => match &tag.target {
            BonsaiAnnotatedTagTarget::Changeset(cs) => *cs,
            // Never produced by production writers, but guard defensively.
            BonsaiAnnotatedTagTarget::Content(_) => return Ok(None),
        },
        None => return Ok(None),
    };
    // Content/unresolved tags store an all-zeros target: there is no commit to
    // move the bookmark to, so they are never diverged (and moving a bookmark
    // there would corrupt the repo).
    if mapping_target == placeholder_target() {
        return Ok(None);
    }
    Ok(match bookmark {
        Some(b) if b != mapping_target => Some(Divergence {
            tag_name: entry.tag_name,
            bookmark_key,
            mapping_target,
            bookmark: b,
        }),
        _ => None,
    })
}

/// Bulk-resolve the git shas of every changeset referenced by `diverged`, for
/// human-readable before->after output.
async fn git_sha_map(
    repo: &Repo,
    ctx: &CoreContext,
    diverged: &[Divergence],
) -> Result<HashMap<ChangesetId, GitSha1>> {
    let ids: Vec<ChangesetId> = diverged
        .iter()
        .flat_map(|d| [d.bookmark, d.mapping_target])
        .collect();
    let entries = repo
        .bonsai_git_mapping()
        .get(ctx, BonsaisOrGitShas::Bonsai(ids))
        .await
        .context("Failed to resolve git shas for diverged tags")?;
    Ok(entries
        .into_iter()
        .map(|entry| (entry.bcs_id, entry.git_sha1))
        .collect())
}

/// Refuse to move tag bookmarks in a push-redirected / megarepo-synced repo,
/// mirroring the guard the git push path enforces -- moving them directly could
/// desync the large repo. Push-redirect is repo-level for publishing `tags/*`
/// bookmarks, so a single representative key suffices.
async fn check_apply_allowed(
    repo: &Repo,
    ctx: &CoreContext,
    sample_tag: &BookmarkKey,
) -> Result<()> {
    check_bookmark_sync_config(ctx, repo, sample_tag, BookmarkKind::Publishing)
        .await
        .context(
            "Refusing --apply: repo is push-redirected / megarepo-synced; moving tag \
             bookmarks directly could desync the large repo. Pass --force to override.",
        )
}

pub async fn reconcile(repo: &Repo, ctx: &CoreContext, args: ReconcileArgs) -> Result<()> {
    let entries = repo
        .bonsai_tag_mapping()
        .get_all_entries(ctx)
        .await
        .context("Failed to fetch bonsai_tag_mapping entries")?;

    // Phase 1: scan for candidates (this read may hit a replica / cache).
    let mut diverged: Vec<Divergence> = stream::iter(entries)
        .map(|entry| divergence_for(repo, ctx, entry))
        .buffered(CONCURRENCY)
        .try_filter_map(|d| async move { Ok(d) })
        .try_collect()
        .await?;
    diverged.sort_by(|a, b| a.tag_name.cmp(&b.tag_name));

    if diverged.is_empty() {
        println!("No diverged bonsai_tag_mapping rows found.");
        return Ok(());
    }

    // Restrict to the --tag allowlist if provided (matched against the full
    // bonsai_tag_mapping name, e.g. `tags/3.4.14475`).
    if !args.tag.is_empty() {
        let allow: HashSet<&str> = args.tag.iter().map(String::as_str).collect();
        diverged.retain(|d| allow.contains(d.tag_name.as_str()));
        if diverged.is_empty() {
            println!("No diverged tags match --tag {:?}.", args.tag);
            return Ok(());
        }
    }

    let git_shas = git_sha_map(repo, ctx, &diverged).await?;
    let show = |cs: &ChangesetId| {
        git_shas
            .get(cs)
            .map(|sha| sha.to_hex().to_string())
            .unwrap_or_else(|| format!("{cs} (no git sha)"))
    };
    for d in &diverged {
        println!(
            "DIVERGED {}: move bookmark {} -> {}",
            d.tag_name,
            show(&d.bookmark),
            show(&d.mapping_target),
        );
    }

    if !args.apply {
        println!(
            "{} diverged tag(s) (dry-run). To recover, re-run with --apply plus either \
             --tag <names> (recommended: the confirmed diverged tags from above) or --all. A \
             divergence can also be a legitimate annotated->lightweight conversion, which \
             recovery would wrongly revert.",
            diverged.len()
        );
        return Ok(());
    }

    // --apply requires an explicit target selection so we never blindly revert a
    // legitimate annotated->lightweight conversion.
    if args.tag.is_empty() && !args.all {
        anyhow::bail!(
            "--apply requires an explicit target: pass --tag <names> (the confirmed diverged \
             tags from the dry-run output) or --all to act on every diverged tag."
        );
    }

    // Repo-level safety guards (unless --force): never move bookmarks in a repo
    // Mononoke does not own, or that is push-redirected / megarepo-synced.
    if !args.force {
        check_apply_allowed(repo, ctx, &diverged[0].bookmark_key).await?;
    }

    // Phase 2: re-verify each candidate against master immediately before moving,
    // so a stale phase-1 read (replica lag) cannot make us move a healthy
    // bookmark. The transaction's old-value compare-and-swap is the final guard
    // against a concurrent re-point.
    let mut recovered = 0usize;
    for d in &diverged {
        let latest = repo
            .bonsai_tag_mapping()
            .get_entry_by_tag_name(ctx, d.tag_name.clone(), Freshness::Latest)
            .await?;
        let current = match latest {
            Some(entry) => divergence_for(repo, ctx, entry).await?,
            None => None,
        };
        let Some(current) = current else {
            println!("SKIP {} (no longer diverged on master)", d.tag_name);
            continue;
        };

        // Never move a bookmark to a target git cannot advertise (no git sha) --
        // that would re-introduce the clone failure this tool repairs.
        if repo
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(ctx, current.mapping_target)
            .await?
            .is_none()
        {
            println!(
                "SKIP {} (annotated tag target {} has no git sha)",
                current.tag_name, current.mapping_target
            );
            continue;
        }

        let mut transaction = repo.bookmarks().create_transaction(ctx.clone());
        // Raw value compare-and-swap: no fast-forward enforcement, so the sibling
        // (non-fast-forward) move from the wrong target back to the annotated
        // tag's target is allowed. It fails cleanly if the bookmark no longer
        // points at `current.bookmark`.
        transaction.update(
            &current.bookmark_key,
            current.mapping_target,
            current.bookmark,
            BookmarkUpdateReason::ManualMove,
        )?;
        match transaction
            .commit()
            .await
            .with_context(|| format!("Failed to move bookmark for tag {}", current.tag_name))?
        {
            Some(_) => {
                let info = BookmarkInfo {
                    bookmark_name: current.bookmark_key.clone(),
                    bookmark_kind: BookmarkKind::Publishing,
                    operation: BookmarkOperation::Update(current.bookmark, current.mapping_target),
                    reason: BookmarkUpdateReason::ManualMove,
                };
                log_bookmark_operation(ctx, repo, &info).await;
                recovered += 1;
            }
            None => println!(
                "SKIP {} (bookmark changed concurrently; compare-and-swap failed)",
                current.tag_name
            ),
        }
    }
    println!("Recovered {recovered} annotated tag(s) by moving the bookmark to the tag target.");
    Ok(())
}
