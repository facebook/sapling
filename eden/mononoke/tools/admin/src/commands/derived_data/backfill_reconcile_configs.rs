/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `mononoke_admin derived-data backfill-reconcile-configs`
//!
//! The Phase-A bridge (design §5.3): reconcile the `enabled_derived_data_types`
//! DB table INTO configerator. The `MarkTypeEnabled` node writes a row the moment
//! a repo's backfill completes; services still read enabled types from
//! configerator, so this manually-run tool creates the corresponding config edits
//! as peer-reviewed configerator diffs, in size-bounded batches.
//!
//! It is **stateless** (design option c): each run re-derives what is pending by
//! comparing the DB rows against the repos' current configs. A type already present
//! in a repo's active config's `types` is skipped — this is what makes the tool
//! idempotent and marker-free. There is no DB write-back after a land; the next run
//! simply sees the type now in config and skips it.
//!
//! Default behavior is a dry-run that prints the plan. Creating the configerator
//! review diff(s) requires `--apply`; each batch becomes one Phabricator diff
//! (always reviewed by the `#mononoke` group) that a reviewer must accept and land
//! — nothing lands automatically, so peer review is the safety gate. (Direct
//! reviewless landing of these `RepoSpec` configs is only authorized for the SCS
//! service identity in the repos `AUTOMATION_ACL`, not for a human running this
//! CLI.)
//!
//! Every run also reports drift between the fleet and the create_repos template
//! `scm/mononoke/repos/common/default_git_repo_spec` (the `RepoSpec` that SCS
//! `create_repos` copies into every new Git repo): derived-data types enabled on
//! at least half the Git repos that the template lacks. The template is never
//! edited implicitly; `--enable-for-new-repos <types>` names what to add, and with
//! `--apply` that edit becomes its own review diff, created after the fleet
//! batches.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use enabled_derived_data_types::EnabledDerivedDataTypesRef;
use metaconfig_types::CommitIdentityScheme;
use metaconfig_types::DerivedDataConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::ConfigArgs;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;

use super::enabled_types::EnabledTypesRepo;

/// Minimal container to reach the (global) `enabled_derived_data_types` facet
/// without opening the heavy `derived-data` container (Gotcha 1: opening many
/// metadata-sqlite facets and then reading the same on-disk sqlite file
/// self-locks). We reuse the `enabled-types` commands' minimal container.
type ReconcileRepo = EnabledTypesRepo;

#[derive(Args)]
pub(super) struct BackfillReconcileConfigsArgs {
    /// Create the configerator review diff(s). Without this flag the command only
    /// prints the plan (dry-run) and mutates nothing. Each batch becomes one
    /// Phabricator diff that a reviewer must accept and land — nothing lands
    /// automatically, so peer review is the safety gate.
    #[clap(long)]
    apply: bool,

    /// Additional reviewers for the configerator review diff(s) created by
    /// `--apply` (comma-separated usernames). The `#mononoke` group is always
    /// added as a reviewer; this flag is optional.
    #[clap(long, value_delimiter = ',')]
    reviewers: Vec<String>,

    /// Maximum number of repos to include in a single configerator land.
    #[clap(long, default_value_t = 1000)]
    batch_size: usize,

    /// Per-type derivation batch size to write into each repo's config
    /// (`derivation_batch_sizes[<type>]`) when enabling a type that has no batch
    /// size set yet. Existing entries are left unchanged. Defaults to 20 — the
    /// same value Mononoke assumes when a type is absent from the map.
    #[clap(long, default_value_t = 20)]
    derivation_batch_size: i64,

    /// Diagnostic: print the derived-data config this CLI resolves for a single
    /// repo id (its `enabled_config_name` and the active config's `types`), then
    /// exit without scanning. Use it to check whether a canaried `.cconf` is
    /// actually being picked up (compare the printed `types` against canary vs
    /// landed).
    #[clap(long)]
    dump_repo_config: Option<i32>,

    /// Also add these derived-data types (by name, e.g. `fastlog_v2`) to the
    /// new-repo template `scm/mononoke/repos/common/default_git_repo_spec`,
    /// as its own review diff created after the fleet batches. Explicit on
    /// purpose: the enablement table says which existing repos were
    /// backfilled, not what a new repo should get. Superseded types (GDM v2
    /// once the template selects v3) are refused.
    #[clap(long, value_delimiter = ',', value_parser = DerivableType::from_name)]
    enable_for_new_repos: Vec<DerivableType>,
}

/// The per-repo facts reconciliation needs: what it's called, which commit
/// identity scheme it uses (which decides where its `.cconf` lives), and what
/// its derived-data config currently enables.
pub(crate) struct RepoReconcileInfo {
    pub(crate) repo_name: String,
    pub(crate) commit_identity_scheme: CommitIdentityScheme,
    pub(crate) derived_data_config: DerivedDataConfig,
}

/// One unit of pending reconciliation work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingReconcile {
    pub(crate) repo_id: RepositoryId,
    pub(crate) repo_name: String,
    pub(crate) derived_data_type: DerivableType,
    /// The active derived-data config name for this repo (the config whose
    /// `types` list gates derivation, and the one the land must edit).
    pub(crate) enabled_config_name: String,
    /// Decides which RepoSpec tree this repo's `.cconf` is edited in:
    /// configerator splits them into `repos/git/` and `repos/hg/`. Carried
    /// per-repo rather than assumed — assuming git sent every hg repo's edit
    /// to a `repos/git/...` path that doesn't exist.
    pub(crate) commit_identity_scheme: CommitIdentityScheme,
}

/// Outcome of comparing the enablement rows against the repos' configs.
#[derive(Debug, Default)]
pub(crate) struct WorkList {
    /// Rows whose type is not yet in the repo's active config — these need a land.
    pub(crate) pending: Vec<PendingReconcile>,
    /// Number of rows skipped because the type is already in the repo's config.
    pub(crate) already_in_config: usize,
    /// Repo ids that have an enablement row but no entry in the loaded configs
    /// (deduped). A non-empty list points at a config-resolution gap, not "done".
    pub(crate) repo_not_found: Vec<RepositoryId>,
}

/// Compute the pending work list from the enablement rows and the repo configs.
///
/// For each `(repo_id, derived_data_type)` enablement row: look up the repo's
/// active config = `derived_data_config.available_configs[enabled_config_name]`;
/// if `derived_data_type` is NOT already in that config's `types`, it is pending.
/// Rows whose type is already in config are counted as `already_in_config`. Rows
/// for a repo_id not present in the configs map are recorded in `repo_not_found`.
pub(crate) fn compute_work_list(
    enablement_rows: Vec<(RepositoryId, DerivableType)>,
    repo_configs: &BTreeMap<RepositoryId, RepoReconcileInfo>,
) -> WorkList {
    let mut work = WorkList::default();
    for (repo_id, ddt) in enablement_rows {
        let Some(info) = repo_configs.get(&repo_id) else {
            work.repo_not_found.push(repo_id);
            continue;
        };

        let ddc = &info.derived_data_config;
        let enabled_config_name = ddc.enabled_config_name.clone();
        let already_enabled = ddc
            .available_configs
            .get(&enabled_config_name)
            .is_some_and(|cfg| cfg.types.contains(&ddt));

        if already_enabled {
            work.already_in_config += 1;
        } else {
            work.pending.push(PendingReconcile {
                repo_id,
                repo_name: info.repo_name.clone(),
                derived_data_type: ddt,
                enabled_config_name,
                commit_identity_scheme: info.commit_identity_scheme.clone(),
            });
        }
    }

    // Deterministic ordering for stable dry-run output and stable batching.
    work.pending.sort_by(|a, b| {
        (a.repo_id, a.derived_data_type.name()).cmp(&(b.repo_id, b.derived_data_type.name()))
    });
    work.repo_not_found.sort();
    work.repo_not_found.dedup();
    work
}

pub(super) async fn backfill_reconcile_configs(
    ctx: &CoreContext,
    app: &MononokeApp,
    args: BackfillReconcileConfigsArgs,
) -> Result<()> {
    // Diagnostic: dump one repo's resolved derived-data config and exit. Reads the
    // per-repo ConfigHandle (the same live, canary-aware path batch_load uses for an
    // uncached repo), so it prints exactly what config this CLI sees for the repo.
    if let Some(repo_id) = args.dump_repo_config {
        let (name, config) = app.configs().get_or_load_repo_config_by_id(repo_id)?;
        let ddc = &config.derived_data_config;
        println!(
            "repo_id={} repo_name={} enabled_config_name={}",
            repo_id, name, ddc.enabled_config_name,
        );
        match ddc.available_configs.get(&ddc.enabled_config_name) {
            Some(active) => {
                let mut types: Vec<String> =
                    active.types.iter().map(|t| t.name().to_string()).collect();
                types.sort();
                println!("active config types: [{}]", types.join(", "));
            }
            None => println!(
                "active config '{}' is not present in available_configs (keys: {:?})",
                ddc.enabled_config_name,
                ddc.available_configs.keys().collect::<Vec<_>>(),
            ),
        }
        return Ok(());
    }

    // Map repo_id -> (repo_name, DerivedDataConfig) for every repo.
    //
    // `load_all_repo_configs()` (not the static `app.repo_configs().repos`) is
    // required: split-loaded services skip deep-sharded repos in the eager map,
    // so those repos would be absent and their enablement rows wrongly treated as
    // "unknown repo" and skipped. `load_all_repo_configs()` unions the eager map
    // with the full tier manifest and materializes each deep-sharded repo's
    // config on demand.
    let repo_configs: BTreeMap<RepositoryId, RepoReconcileInfo> = app
        .configs()
        .load_all_repo_configs()?
        .into_iter()
        .map(|(name, config)| {
            (
                config.repoid,
                RepoReconcileInfo {
                    repo_name: name,
                    commit_identity_scheme: config.default_commit_identity_scheme,
                    derived_data_config: config.derived_data_config,
                },
            )
        })
        .collect();

    // Reach the global enabled-types facet via a minimal container (Gotcha 1).
    // The table is global, so any configured repo handle works; open the
    // lowest-id repo for determinism.
    let first_repo_id = repo_configs
        .keys()
        .next()
        .copied()
        .context("no repos are configured")?;
    let repo: ReconcileRepo = app.open_named_repo(first_repo_id).await?;

    let enablement_rows: Vec<(RepositoryId, DerivableType)> = repo
        .enabled_derived_data_types()
        .get_all(ctx)
        .await
        .context("reading enabled_derived_data_types table")?
        .into_iter()
        .map(|entry| (entry.repo_id, entry.derived_data_type))
        .collect();

    let work = compute_work_list(enablement_rows, &repo_configs);

    println!(
        "Scanned {} enablement row(s): {} pending, {} already in config, {} repo(s) not in loaded configs.",
        work.pending.len() + work.already_in_config + work.repo_not_found.len(),
        work.pending.len(),
        work.already_in_config,
        work.repo_not_found.len(),
    );
    if !work.repo_not_found.is_empty() {
        let shown: Vec<i32> = work
            .repo_not_found
            .iter()
            .take(20)
            .map(|r| r.id())
            .collect();
        println!(
            "  repo(s) with an enablement row but no loaded config (skipped): {:?}{}",
            shown,
            if work.repo_not_found.len() > 20 {
                " (...truncated)"
            } else {
                ""
            },
        );
    }

    // New-repo template: the drift line on every run; plan lines when the flag
    // is set.
    let template = load_template_for_drift(app);
    let config_source = app.args::<ConfigArgs>()?.config_path();
    if let Some(t) = template.as_ref() {
        let d = template_drift(&repo_configs, t);
        if !d.lacking.is_empty() {
            println!(
                "new-repo template drift (template: {DEFAULT_GIT_REPO_SPEC_PATH_STR}; \
                 fleet: {} git repos from {config_source}):",
                d.git_total
            );
            for (ty, n) in &d.lacking {
                println!(
                    "  lacks `{}` (enabled on {n} of {} git repos); \
                     pass --enable-for-new-repos {} to add it",
                    ty.name(),
                    d.git_total,
                    ty.name()
                );
            }
        }
    }

    let pending = work.pending;
    let batches: Vec<&[PendingReconcile]> = pending.chunks(args.batch_size.max(1)).collect();
    if pending.is_empty() {
        println!(
            "Nothing to reconcile for existing repos: all enabled types are already present in config."
        );
    } else if !args.apply {
        print_plan(&batches);
    }

    let template_plan = if args.enable_for_new_repos.is_empty() {
        Vec::new()
    } else {
        let t = template.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "new-repo template not loadable from {config_source}; \
                 refusing --enable-for-new-repos"
            )
        })?;
        plan_template_edits(t, &args.enable_for_new_repos)?
    };
    if let Some(t) = template.as_ref() {
        for item in &template_plan {
            match item {
                TemplatePlanItem::Add(ty) => println!(
                    "template: add `{}` to config `{}`",
                    ty.name(),
                    t.enabled_config_name
                ),
                TemplatePlanItem::AlreadyPresent(ty) => {
                    println!("template already has `{}`; nothing to do", ty.name())
                }
            }
        }
    }

    if !args.apply {
        // Only worth saying when a re-run with --apply would do something.
        if !pending.is_empty() || template_plan.iter().any(TemplatePlanItem::is_add) {
            println!(
                "\nDry run: no configerator changes were made. Re-run with --apply \
                 to create review diff(s)."
            );
        }
        return Ok(());
    }

    // The `#mononoke` group always reviews these config changes; user-supplied
    // reviewers are added on top.
    let mut reviewers: BTreeSet<String> = args.reviewers.iter().cloned().collect();
    reviewers.insert("#mononoke".to_string());
    if !pending.is_empty() {
        apply_batches(ctx, &batches, &reviewers, args.derivation_batch_size).await?;
    }
    if template_plan.iter().any(TemplatePlanItem::is_add) {
        match apply_template(ctx, &template_plan, &reviewers, args.derivation_batch_size).await? {
            Some(diff) => println!("Created review diff {diff} for the new-repo template."),
            None => println!("template: no effective edits; no diff created."),
        }
    }
    Ok(())
}

fn print_plan(batches: &[&[PendingReconcile]]) {
    let total: usize = batches.iter().map(|b| b.len()).sum();
    println!(
        "Reconciliation plan: {} pending (repo, type) enablement(s) across {} batch(es):",
        total,
        batches.len(),
    );
    for (i, batch) in batches.iter().enumerate() {
        println!("Batch {} ({} repos):", i + 1, batch.len());
        for p in batch.iter() {
            println!(
                "  repo_id={} repo_name={} type={} config={}",
                p.repo_id.id(),
                p.repo_name,
                p.derived_data_type.name(),
                p.enabled_config_name,
            );
        }
    }
}

/// The new-repo template reduced to what drift and the flag path need. Built
/// from the raw `repos::RepoSpec` inside `mod fb` (the raw types are
/// fbcode-only deps); kept here so the rules are unit-testable everywhere.
/// Only constructed from `mod fb` and `mod tests`, hence the allow below.
#[cfg_attr(not(fbcode_build), allow(dead_code))]
pub(crate) struct TemplateDriftInput {
    /// The template's own enabled variant name (`default` today). The flag
    /// path edits only this variant.
    pub(crate) enabled_config_name: String,
    pub(crate) types: BTreeSet<DerivableType>,
    pub(crate) git_delta_manifest_version: Option<i16>,
}

/// Version selector at or below which GDM v2 is the live format. The parser
/// accepts None (V2), 2 and 3 today (metaconfig/parser/src/convert/repo.rs);
/// "newer than v2" stays correct when a v4 arrives.
const GDM_V2_VERSION: i16 = 2;

/// First half of a two-place pin with `FORBIDDEN_NEW_REPO_DERIVED_TYPES` in
/// configerator `repos/repo_spec.ctest`; change both together. Mirrors
/// `ensure_required_tuning`'s version selector: once the template selects
/// something newer than GDM v2, GDM v2 is superseded there and is neither
/// drift nor writable by `--enable-for-new-repos`.
pub(crate) fn superseded_in_template(ty: DerivableType, t: &TemplateDriftInput) -> bool {
    matches!(ty, DerivableType::GitDeltaManifestsV2)
        && t.git_delta_manifest_version
            .is_some_and(|v| v > GDM_V2_VERSION)
}

/// What the fleet has that the new-repo template lacks.
pub(crate) struct TemplateDrift {
    pub(crate) git_total: usize,
    /// Types in the enabled config of at least half the GIT repos, absent from
    /// the template and not superseded there. Sorted by type name.
    pub(crate) lacking: Vec<(DerivableType, usize)>,
}

pub(crate) fn template_drift(
    repo_configs: &BTreeMap<RepositoryId, RepoReconcileInfo>,
    template: &TemplateDriftInput,
) -> TemplateDrift {
    let mut git_total = 0usize;
    let mut counts: BTreeMap<&'static str, (DerivableType, usize)> = BTreeMap::new();
    for info in repo_configs.values() {
        if info.commit_identity_scheme != CommitIdentityScheme::GIT {
            continue;
        }
        git_total += 1;
        let ddc = &info.derived_data_config;
        let Some(active) = ddc.available_configs.get(&ddc.enabled_config_name) else {
            continue;
        };
        for ty in &active.types {
            counts.entry(ty.name()).or_insert((*ty, 0)).1 += 1;
        }
    }
    let lacking = counts
        .into_values()
        .filter(|(ty, n)| {
            if git_total == 0 || *n * 2 < git_total || template.types.contains(ty) {
                return false;
            }
            if superseded_in_template(*ty, template) {
                tracing::debug!(
                    "new-repo template drift: `{}` is on {n} of {git_total} git repos \
                     but superseded on the template; not reported",
                    ty.name()
                );
                return false;
            }
            true
        })
        .collect();
    TemplateDrift { git_total, lacking }
}

/// What `--enable-for-new-repos` would do to the template, per type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TemplatePlanItem {
    Add(DerivableType),
    AlreadyPresent(DerivableType),
}

impl TemplatePlanItem {
    pub(crate) fn is_add(&self) -> bool {
        matches!(self, Self::Add(_))
    }
}

/// The single classification both the dry run and `--apply` consume, so the
/// two cannot disagree. A superseded type is a usage error, before any
/// Configo call.
pub(crate) fn plan_template_edits(
    template: &TemplateDriftInput,
    types: &[DerivableType],
) -> Result<Vec<TemplatePlanItem>> {
    // A name repeated on the command line is one request, not two.
    let mut seen = BTreeSet::new();
    types
        .iter()
        .filter(|ty| seen.insert(**ty))
        .map(|ty| {
            if superseded_in_template(*ty, template) {
                anyhow::bail!(
                    "`{}` is superseded on the new-repo template \
                     (git_delta_manifest_version={:?}); refusing. If you really need it, \
                     edit the template and FORBIDDEN_NEW_REPO_DERIVED_TYPES in \
                     repos/repo_spec.ctest by hand.",
                    ty.name(),
                    template.git_delta_manifest_version,
                );
            }
            Ok(if template.types.contains(ty) {
                TemplatePlanItem::AlreadyPresent(*ty)
            } else {
                TemplatePlanItem::Add(*ty)
            })
        })
        .collect()
}

#[cfg_attr(not(fbcode_build), allow(dead_code))]
fn names(types: &[DerivableType]) -> String {
    types
        .iter()
        .map(|t| format!("`{}`", t.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Title of the template's review diff. One file is edited, so unlike the
/// fleet batches it needs no size-limit bypass.
#[cfg_attr(not(fbcode_build), allow(dead_code))]
pub(crate) fn template_review_diff_title(added: &[DerivableType]) -> String {
    format!(
        "[mononoke]: Enable {} for new Git repos (create_repos template)",
        names(added)
    )
}

#[cfg_attr(not(fbcode_build), allow(dead_code))]
pub(crate) fn template_review_diff_summary(
    added: &[DerivableType],
    enabled_config_name: &str,
) -> String {
    format!(
        "Enables {} for newly created Git repos.\n\
         \n\
         Edits `scm/mononoke/repos/common/default_git_repo_spec`, the RepoSpec template that \
         SCS `create_repos` copies into every new Git repo. It is not a repo: never indexed, \
         never in a manifest, never served. The type(s) are added to its enabled derived-data \
         config `{}`; existing repos are not touched by this diff (see the fleet batches \
         created by the same run).",
        names(added),
        enabled_config_name,
    )
}

#[cfg_attr(not(fbcode_build), allow(dead_code))]
pub(crate) fn template_review_diff_test_plan() -> String {
    "Created by `mononoke_admin derived-data backfill-reconcile-configs \
     --enable-for-new-repos ... --apply`.\n\
     \n\
     - Configerator's `prepare` compiled the template and its dependent ctest server-side \
     before this diff was published; the policy pins in `repos/repo_spec.ctest` \
     (required/forbidden types, single variant, GDM v3 tuning) passed.\n\
     - The tool is idempotent: a type already in the template is reported and skipped."
        .to_string()
}

/// The template's own pending entry. `enabled_config_name` comes from the
/// template itself, so the flag path can only ever edit the variant Mononoke
/// reads; editing a variant that is not the enabled one is impossible by
/// construction.
#[cfg_attr(not(fbcode_build), allow(dead_code))]
pub(crate) fn template_pending_reconcile(
    repo_id: i32,
    repo_name: &str,
    enabled_config_name: &str,
    ty: DerivableType,
) -> PendingReconcile {
    PendingReconcile {
        repo_id: RepositoryId::new(repo_id),
        repo_name: repo_name.to_string(),
        derived_data_type: ty,
        enabled_config_name: enabled_config_name.to_string(),
        commit_identity_scheme: CommitIdentityScheme::GIT,
    }
}

/// Configerator path of the new-repo template. Duplicates
/// `repo_spec_writer::DEFAULT_GIT_REPO_SPEC_PATH` because that crate is an
/// fbcode-only dependency of this binary (tools/admin/BUCK) and the drift
/// header is printed from unconditional code;
/// `template_path_matches_repo_spec_writer` pins the two equal.
pub(crate) const DEFAULT_GIT_REPO_SPEC_PATH_STR: &str =
    "scm/mononoke/repos/common/default_git_repo_spec";

#[cfg(fbcode_build)]
fn load_template_for_drift(app: &MononokeApp) -> Option<TemplateDriftInput> {
    match fb::load_template(app) {
        Ok(t) => Some(t),
        Err(e) => {
            println!("warning: new-repo template drift check skipped: {e:#}");
            None
        }
    }
}

#[cfg(not(fbcode_build))]
fn load_template_for_drift(_app: &MononokeApp) -> Option<TemplateDriftInput> {
    // Same prefix as the fbcode variant so one expectation covers both builds.
    println!("warning: new-repo template drift check skipped: not available in non-fbcode builds");
    None
}

#[cfg(fbcode_build)]
async fn apply_template(
    ctx: &CoreContext,
    plan: &[TemplatePlanItem],
    reviewers: &BTreeSet<String>,
    derivation_batch_size: i64,
) -> Result<Option<String>> {
    fb::apply_template(ctx, plan, reviewers, derivation_batch_size).await
}

#[cfg(not(fbcode_build))]
async fn apply_template(
    _ctx: &CoreContext,
    _plan: &[TemplatePlanItem],
    _reviewers: &BTreeSet<String>,
    _derivation_batch_size: i64,
) -> Result<Option<String>> {
    Err(anyhow::Error::msg(
        "configo is not available in non-fbcode builds; --apply cannot create config diffs",
    ))
}

#[cfg(fbcode_build)]
async fn apply_batches(
    ctx: &CoreContext,
    batches: &[&[PendingReconcile]],
    reviewers: &BTreeSet<String>,
    derivation_batch_size: i64,
) -> Result<()> {
    for (i, batch) in batches.iter().enumerate() {
        tracing::debug!(
            "creating review diff for reconcile batch {} of {}",
            i + 1,
            batches.len()
        );
        match fb::create_review_diff(ctx, batch, reviewers, derivation_batch_size)
            .await
            .with_context(|| format!("creating review diff for reconcile batch {}", i + 1))?
        {
            Some(diff) => println!(
                "Created review diff {} for batch {}/{} ({} repos).",
                diff,
                i + 1,
                batches.len(),
                batch.len(),
            ),
            None => println!(
                "Batch {}/{} had no effective edits; no diff created.",
                i + 1,
                batches.len(),
            ),
        }
    }
    println!(
        "\nReview diff(s) created. Each requires peer review; a reviewer must accept \
         and land it before the config changes take effect."
    );
    Ok(())
}

#[cfg(not(fbcode_build))]
async fn apply_batches(
    _ctx: &CoreContext,
    _batches: &[&[PendingReconcile]],
    _reviewers: &BTreeSet<String>,
    _derivation_batch_size: i64,
) -> Result<()> {
    Err(anyhow::Error::msg(
        "configo is not available in non-fbcode builds; --apply cannot create config diffs",
    ))
}

/// The configerator-touching path. Gated to fbcode builds; builds the same
/// `RepoSpec` mutation as `servers/scs/scs_methods/src/methods/create_repos.rs`
/// (`prepare_repo_configs_mutation_nowait`), corrected to the RepoSpec scheme per
/// spike U1/U3, but publishes it as a peer-review Phabricator diff instead of
/// landing directly — direct reviewless landing of these configs is only
/// authorized for the SCS service identity in the repos `AUTOMATION_ACL`.
#[cfg(fbcode_build)]
mod fb {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::time::Duration;

    use anyhow::Result;
    use anyhow::anyhow;
    use anyhow::bail;
    use configo::ConfigoClient;
    use configo_thrift_srclients::make_ConfigoService_srclient;
    use context::CoreContext;
    use metaconfig_parser::configerator_repo_spec_handle;
    use metaconfig_types::CommitIdentityScheme;
    use mononoke_app::MononokeApp;
    use mononoke_types::DerivableType;
    use repo_spec_writer::DEFAULT_GIT_REPO_SPEC_PATH;
    use repo_spec_writer::RepoSpecDir;
    use repo_spec_writer::default_git_repo_spec_file_path;
    use repo_spec_writer::make_repo_spec_file_path;
    use repos::RawDerivedDataTypesConfig;
    use repos::RepoSpec;

    use super::PendingReconcile;
    use super::TemplateDriftInput;
    use super::TemplatePlanItem;
    use super::superseded_in_template;
    use super::template_pending_reconcile;
    use super::template_review_diff_summary;
    use super::template_review_diff_test_plan;
    use super::template_review_diff_title;

    const REPO_SPEC_THRIFT_TYPE: &str = "RepoSpec";
    const REPO_SPEC_THRIFT_PATH: &str = "source/scm/mononoke/repos/repos.thrift";
    // Configerator prepare compiles the edited configs server-side; allow ample time.
    const PREPARE_TIMEOUT: Duration = Duration::from_secs(600);
    // i16 selector on RawDerivedDataTypesConfig.git_delta_manifest_version; 3 => V3.
    const GDM_V3_VERSION: i16 = 3;

    /// Which RepoSpec tree this repo's `.cconf` lives in.
    ///
    /// Configerator splits per-repo configs into `repos/git/` and `repos/hg/`,
    /// with identical `sha256(repo_name)` sharding inside each. Only GIT and HG
    /// occur in practice — verified against every repo in `repo_index.cinc`:
    /// 9,938 GIT under `repos/git/`, 62 HG under `repos/hg/`, no exceptions.
    /// BONSAI/UNKNOWN have no tree of their own, so guessing one would silently
    /// aim the edit at a nonexistent file; fail loudly instead.
    pub(super) fn repo_spec_dir_for(p: &PendingReconcile) -> Result<RepoSpecDir> {
        match p.commit_identity_scheme {
            CommitIdentityScheme::GIT => Ok(RepoSpecDir::Git),
            CommitIdentityScheme::HG => Ok(RepoSpecDir::Hg),
            ref other => bail!(
                "repo {} ({}) has commit identity scheme {other:?}, which has no \
                 RepoSpec directory (expected GIT or HG); refusing to guess its .cconf path",
                p.repo_id.id(),
                p.repo_name,
            ),
        }
    }

    /// How many individual repos the review diff's summary names. A batch carries up
    /// to `--batch-size` repos (1000 by default) and the diff's own changed files
    /// already enumerate every one of them, so repeating the full list in the message
    /// only buries the per-type breakdown a reviewer actually reads.
    const MAX_LISTED_REPOS: usize = 20;

    /// Above this many distinct types the prose names a count instead of listing
    /// them, so the title stays a readable single line.
    const MAX_NAMED_TYPES: usize = 3;

    /// How many repos each derived data type is being enabled for, type-ordered.
    fn counts_by_type(edits: &[&PendingReconcile]) -> BTreeMap<&'static str, usize> {
        edits.iter().fold(BTreeMap::new(), |mut counts, p| {
            *counts.entry(p.derived_data_type.name()).or_default() += 1;
            counts
        })
    }

    /// The types being enabled, named when there are few enough to fit on one line.
    fn types_phrase(counts: &BTreeMap<&'static str, usize>) -> String {
        if counts.len() <= MAX_NAMED_TYPES {
            counts
                .keys()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            format!("{} derived data types", counts.len())
        }
    }

    /// Title of the review diff. `@bypass_size_limit` is required because a batch
    /// edits up to `--batch-size` `.cconf` files at once.
    fn review_diff_title(edits: &[&PendingReconcile]) -> String {
        format!(
            "[mononoke]: Enable {} for {} repo(s) (automated backfill reconcile)\n@bypass_size_limit",
            types_phrase(&counts_by_type(edits)),
            edits.len(),
        )
    }

    /// Summary of the review diff: what the change does, a per-type breakdown, and a
    /// sample of the affected repos capped at `MAX_LISTED_REPOS`.
    fn review_diff_summary(edits: &[&PendingReconcile]) -> String {
        let counts = counts_by_type(edits);
        let types = types_phrase(&counts);
        let type_rows: String = counts
            .iter()
            .map(|(name, count)| format!("| `{name}` | {count} |\n"))
            .collect();

        let repo_rows: String = edits
            .iter()
            .take(MAX_LISTED_REPOS)
            .map(|p| {
                format!(
                    "| {} | `{}` | `{}` | `{}` |\n",
                    p.repo_id.id(),
                    p.repo_name,
                    p.derived_data_type.name(),
                    p.enabled_config_name,
                )
            })
            .collect();

        let repos_heading = if edits.len() > MAX_LISTED_REPOS {
            format!(
                "Affected repos (first {} of {}; this diff's changed files cover all of them):",
                MAX_LISTED_REPOS,
                edits.len(),
            )
        } else {
            "Affected repos:".to_string()
        };

        format!(
            "Enables {types} for {} repo(s).\n\
             \n\
             Automated reconcile of the `enabled_derived_data_types` table into \
             configerator: each repo below has already been backfilled for the type, \
             so this adds the type to that repo's active derived-data config — one \
             `.cconf` edit per repo.\n\
             \n\
             | Derived data type | Repos |\n\
             | --- | --- |\n\
             {type_rows}\
             \n\
             {repos_heading}\n\
             \n\
             | Repo ID | Repo | Type | Active config |\n\
             | --- | --- | --- | --- |\n\
             {repo_rows}",
            edits.len(),
        )
    }

    /// Test plan of the review diff. Deliberately does not repeat the repo list: it
    /// is the diff's changed files.
    fn review_diff_test_plan(edits: &[&PendingReconcile]) -> String {
        format!(
            "Created by `mononoke_admin derived-data backfill-reconcile-configs --apply`.\n\
             \n\
             - Configerator's `prepare` compiled all {} edited `RepoSpec` config(s) \
             server-side before this diff was published, so every `.cconf` edit parses \
             and type-checks.\n\
             - Each edit adds the type to the active config's `types` and ensures the \
             tuning that type requires (its `derivation_batch_sizes` entry, and for \
             GDMV3 the version selector). Nothing else in the config is touched.\n\
             - The tool is idempotent: a repo whose active config already lists the type \
             is skipped, so re-running it produces no further edits.\n\
             \n\
             Per-type breakdown is in the summary; the affected repos are this diff's \
             changed files.",
            edits.len(),
        )
    }

    /// The review path publishes a Phabricator diff, whose author must resolve
    /// to an employee FBID. The `scm_server_infra` service identity does not, so
    /// stamp the diff with the unixname of the human running this CLI instead.
    fn review_author() -> Result<String> {
        std::env::var("USER").map_err(|_| {
            anyhow!(
                "cannot determine your unixname from $USER to author the review diff; \
                 set USER to your unixname and re-run"
            )
        })
    }

    /// Create one peer-review configerator diff covering every repo in `batch`.
    ///
    /// One `managed_transaction`: for each repo read its `RepoSpec` `.cconf`, add
    /// the type to the active config's `types` (idempotent), ensure the type's
    /// required tuning (including its `derivation_batch_sizes` entry) is present,
    /// and write it back; then prepare and publish a Phabricator review diff
    /// (assigned to `reviewers`). Returns the diff id (e.g. `D123`), or `None`
    /// when the batch had no effective edits.
    pub(super) async fn create_review_diff(
        ctx: &CoreContext,
        batch: &[PendingReconcile],
        reviewers: &BTreeSet<String>,
        derivation_batch_size: i64,
    ) -> Result<Option<String>> {
        let configo_client =
            ConfigoClient::with_client(ctx.fb, make_ConfigoService_srclient!(ctx.fb)?);
        let mut txn = configo_client.managed_transaction();

        // The repos this transaction actually changed. Not the same as `batch`:
        // a repo whose config already lists the type is skipped, and the diff
        // message must describe what was edited, not what was attempted.
        let mut edited: Vec<&PendingReconcile> = Vec::new();
        for p in batch {
            let cconf_path = make_repo_spec_file_path(&p.repo_name, repo_spec_dir_for(p)?);

            // Read pins the CAS version for this file. The handle borrows `txn`, so
            // clone the value out and drop the handle before mutating with
            // `set_thrift_object` (CAS-pin caveat from create_repos.rs).
            let repo_spec: RepoSpec = {
                let handle = txn
                    .get_thrift_object::<RepoSpec>(cconf_path.clone())
                    .await?;
                handle.clone()
            };

            match apply_type_to_repo_spec(repo_spec, p, derivation_batch_size)? {
                Some(updated) => {
                    txn.set_thrift_object(
                        updated,
                        cconf_path,
                        REPO_SPEC_THRIFT_TYPE.to_string(),
                        REPO_SPEC_THRIFT_PATH.to_string(),
                        None,
                    );
                    edited.push(p);
                }
                None => {
                    // Type already present in config (raced with a prior land or
                    // manual edit) — nothing to do for this repo.
                    tracing::debug!(
                        "repo {} already has {} in config {}; skipping in-batch",
                        p.repo_id.id(),
                        p.derived_data_type.name(),
                        p.enabled_config_name,
                    );
                }
            }
        }

        if edited.is_empty() {
            tracing::debug!("batch had no effective edits; not creating an empty review diff");
            return Ok(None);
        }

        let author = review_author()?;
        let mutation = txn
            .prepare_mutation_request()?
            .add_author(author)
            .add_commit_message(review_diff_title(&edited), review_diff_summary(&edited))
            .prepare(PREPARE_TIMEOUT)
            .await?;

        let diff = mutation
            .review(reviewers.clone(), review_diff_test_plan(&edited))
            .await?;
        tracing::debug!("created review diff {} for reconcile batch", diff);
        Ok(Some(diff))
    }

    /// Raw RepoSpec -> the reduced drift input. Reused on the Configo-fetched
    /// copy inside `apply_template`. Unknown type names are warned and skipped.
    pub(super) fn reduce_template(spec: &RepoSpec) -> Result<TemplateDriftInput> {
        let ddc = spec
            .repo_config
            .as_ref()
            .ok_or_else(|| anyhow!("new-repo template has no repo_config"))?
            .derived_data_config
            .as_ref()
            .ok_or_else(|| anyhow!("new-repo template has no derived_data_config"))?;
        let name = ddc
            .enabled_config_name
            .as_ref()
            .ok_or_else(|| anyhow!("new-repo template has no enabled_config_name"))?;
        let cfg = ddc
            .available_configs
            .as_ref()
            .and_then(|m| m.get(name))
            .ok_or_else(|| {
                anyhow!("new-repo template enabled config `{name}` is not in available_configs")
            })?;
        let mut types = BTreeSet::new();
        for raw in &cfg.types {
            match DerivableType::from_name(raw) {
                Ok(t) => {
                    types.insert(t);
                }
                Err(_) => tracing::warn!(
                    "new-repo template lists unknown derived-data type `{raw}`; ignored"
                ),
            }
        }
        Ok(TemplateDriftInput {
            enabled_config_name: name.clone(),
            types,
            git_delta_manifest_version: cfg.git_delta_manifest_version,
        })
    }

    /// The template as this CLI's config store sees it (live, canary-aware).
    pub(super) fn load_template(app: &MononokeApp) -> Result<TemplateDriftInput> {
        let spec =
            configerator_repo_spec_handle(DEFAULT_GIT_REPO_SPEC_PATH, app.config_store())?.get();
        reduce_template(&spec)
    }

    /// Create the one peer-review configerator diff that edits the new-repo
    /// template. Same transaction shape as `create_review_diff`, for one file.
    /// Returns the diff id, or `None` when nothing needed adding.
    pub(super) async fn apply_template(
        ctx: &CoreContext,
        plan: &[TemplatePlanItem],
        reviewers: &BTreeSet<String>,
        derivation_batch_size: i64,
    ) -> Result<Option<String>> {
        let configo_client =
            ConfigoClient::with_client(ctx.fb, make_ConfigoService_srclient!(ctx.fb)?);
        let mut txn = configo_client.managed_transaction();
        let template_path = default_git_repo_spec_file_path();
        // Read pins the CAS version; clone out and drop the handle before
        // mutating (same caveat as `create_review_diff`).
        let template: RepoSpec = {
            let handle = txn
                .get_thrift_object::<RepoSpec>(template_path.clone())
                .await?;
            handle.clone()
        };
        // The ConfigStore copy and the Configo (trunk) copy can differ; re-check
        // the copy we are about to write. `fetched` also carries the enabled
        // variant name we edit.
        let fetched = reduce_template(&template)?;
        let (template_repo_id, template_repo_name) = (template.repo_id, template.repo_name.clone());
        let mut spec = template;
        let mut added = Vec::new();
        for item in plan {
            let TemplatePlanItem::Add(ty) = item else {
                continue;
            };
            if superseded_in_template(*ty, &fetched) {
                bail!(
                    "`{}` is superseded on the new-repo template as fetched from Configo; refusing",
                    ty.name()
                );
            }
            let p = template_pending_reconcile(
                template_repo_id,
                &template_repo_name,
                &fetched.enabled_config_name,
                *ty,
            );
            match apply_type_to_repo_spec(spec.clone(), &p, derivation_batch_size)? {
                Some(updated) => {
                    spec = updated;
                    added.push(*ty);
                }
                None => println!("template already has `{}`; nothing to do", ty.name()),
            }
        }
        if added.is_empty() {
            return Ok(None);
        }
        txn.set_thrift_object(
            spec,
            template_path,
            REPO_SPEC_THRIFT_TYPE.to_string(),
            REPO_SPEC_THRIFT_PATH.to_string(),
            None,
        );
        let author = review_author()?;
        // Until the old template file is deleted from configerator, the Configo
        // prepare step rejects this write by a parity test in
        // repos/repo_spec.ctest; that is expected for now and surfaces here.
        let mutation = txn
            .prepare_mutation_request()?
            .add_author(author)
            .add_commit_message(
                template_review_diff_title(&added),
                template_review_diff_summary(&added, &fetched.enabled_config_name),
            )
            .prepare(PREPARE_TIMEOUT)
            .await?;
        let diff = mutation
            .review(reviewers.clone(), template_review_diff_test_plan())
            .await?;
        tracing::debug!("created review diff {} for the new-repo template", diff);
        Ok(Some(diff))
    }

    /// Add `p.derived_data_type` to the active config's `types` in `repo_spec`,
    /// returning the mutated spec, or `None` if the type is already present
    /// (idempotent no-op). Also ensures the type's required tuning block exists.
    fn apply_type_to_repo_spec(
        mut repo_spec: RepoSpec,
        p: &PendingReconcile,
        derivation_batch_size: i64,
    ) -> Result<Option<RepoSpec>> {
        let repo_config = repo_spec.repo_config.as_mut().ok_or_else(|| {
            anyhow!(
                "repo {} ({}) RepoSpec has no repo_config; refusing to fabricate one",
                p.repo_id.id(),
                p.repo_name,
            )
        })?;
        let ddc = repo_config.derived_data_config.as_mut().ok_or_else(|| {
            anyhow!(
                "repo {} ({}) has no derived_data_config; refusing to fabricate one",
                p.repo_id.id(),
                p.repo_name,
            )
        })?;
        let available_configs = ddc.available_configs.as_mut().ok_or_else(|| {
            anyhow!(
                "repo {} ({}) derived_data_config has no available_configs",
                p.repo_id.id(),
                p.repo_name,
            )
        })?;
        let cfg = available_configs
            .get_mut(&p.enabled_config_name)
            .ok_or_else(|| {
                anyhow!(
                    "repo {} ({}) has no available_config named '{}' (its enabled config)",
                    p.repo_id.id(),
                    p.repo_name,
                    p.enabled_config_name,
                )
            })?;

        let type_name = p.derived_data_type.name().to_string();
        if cfg.types.contains(&type_name) {
            return Ok(None);
        }

        ensure_required_tuning(cfg, p, derivation_batch_size)?;
        cfg.types.insert(type_name);
        Ok(Some(repo_spec))
    }

    /// Ensure any tuning a type needs is present in the config. Type-agnostic where
    /// possible: for GDMV3, `git_delta_manifest_version` must be 3 and a
    /// `git_delta_manifest_v3_config` block must exist. Per spike U1 we assert the
    /// tuning block is present rather than fabricating it (fabricating tuning risks
    /// wrong values); only the cheap version selector is set.
    ///
    /// Additionally sets the type's `derivation_batch_sizes` entry to
    /// `derivation_batch_size` if it is not already present. This is only a no-op
    /// at runtime (Mononoke defaults an absent type to 20), but making it explicit
    /// in config keeps the enabled type self-describing. Existing entries are left
    /// untouched.
    fn ensure_required_tuning(
        cfg: &mut RawDerivedDataTypesConfig,
        p: &PendingReconcile,
        derivation_batch_size: i64,
    ) -> Result<()> {
        if p.derived_data_type == mononoke_types::DerivableType::GitDeltaManifestsV3 {
            if cfg.git_delta_manifest_v3_config.is_none() {
                bail!(
                    "repo {} ({}) config '{}' is missing git_delta_manifest_v3_config; refusing to \
                     fabricate GDMV3 tuning — populate it in config first",
                    p.repo_id.id(),
                    p.repo_name,
                    p.enabled_config_name,
                );
            }
            cfg.git_delta_manifest_version = Some(GDM_V3_VERSION);
        }

        cfg.derivation_batch_sizes
            .get_or_insert_with(BTreeMap::new)
            .entry(p.derived_data_type.name().to_string())
            .or_insert(derivation_batch_size);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
