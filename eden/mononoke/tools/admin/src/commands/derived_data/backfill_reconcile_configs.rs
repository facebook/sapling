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

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use enabled_derived_data_types::EnabledDerivedDataTypesRef;
use metaconfig_types::DerivedDataConfig;
use mononoke_app::MononokeApp;
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
    repo_configs: &BTreeMap<RepositoryId, (String, DerivedDataConfig)>,
) -> WorkList {
    let mut work = WorkList::default();
    for (repo_id, ddt) in enablement_rows {
        let Some((repo_name, ddc)) = repo_configs.get(&repo_id) else {
            work.repo_not_found.push(repo_id);
            continue;
        };

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
                repo_name: repo_name.clone(),
                derived_data_type: ddt,
                enabled_config_name,
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
    let repo_configs: BTreeMap<RepositoryId, (String, DerivedDataConfig)> = app
        .configs()
        .load_all_repo_configs()?
        .into_iter()
        .map(|(name, config)| (config.repoid, (name, config.derived_data_config)))
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

    let pending = work.pending;
    if pending.is_empty() {
        println!("Nothing to reconcile: all enabled types are already present in config.");
        return Ok(());
    }

    let batches: Vec<&[PendingReconcile]> = pending.chunks(args.batch_size.max(1)).collect();

    if !args.apply {
        print_plan(&batches);
        println!(
            "\nDry run: no configerator changes were made. Re-run with --apply \
             to create review diff(s)."
        );
        return Ok(());
    }

    // The `#mononoke` group always reviews these config changes; user-supplied
    // reviewers are added on top.
    let mut reviewers: BTreeSet<String> = args.reviewers.iter().cloned().collect();
    reviewers.insert("#mononoke".to_string());
    apply_batches(ctx, &batches, &reviewers, args.derivation_batch_size).await
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
    use repo_spec_writer::make_repo_spec_file_path;
    use repos::RawDerivedDataTypesConfig;
    use repos::RepoSpec;

    use super::PendingReconcile;

    const REPO_SPEC_THRIFT_TYPE: &str = "RepoSpec";
    const REPO_SPEC_THRIFT_PATH: &str = "source/scm/mononoke/repos/repos.thrift";
    // Configerator prepare compiles the edited configs server-side; allow ample time.
    const PREPARE_TIMEOUT: Duration = Duration::from_secs(600);
    // i16 selector on RawDerivedDataTypesConfig.git_delta_manifest_version; 3 => V3.
    const GDMV3_VERSION: i16 = 3;

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

        let mut edited = 0usize;
        for p in batch {
            let cconf_path = make_repo_spec_file_path(&p.repo_name);

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
                    edited += 1;
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

        if edited == 0 {
            tracing::debug!("batch had no effective edits; not creating an empty review diff");
            return Ok(None);
        }

        let summary = batch
            .iter()
            .map(|p| {
                format!(
                    "|{}|{}|{}|",
                    p.repo_id.id(),
                    p.repo_name,
                    p.derived_data_type.name()
                )
            })
            .collect::<String>();
        // The review path publishes a Phabricator diff, whose author must resolve
        // to an employee FBID. The `scm_server_infra` service identity does not, so
        // stamp the diff with the unixname of the human running this CLI instead.
        let author = std::env::var("USER").map_err(|_| {
            anyhow!(
                "cannot determine your unixname from $USER to author the review diff; \
                 set USER to your unixname and re-run"
            )
        })?;
        let mutation = txn
            .prepare_mutation_request()?
            .add_author(author)
            .add_commit_message(
                format!(
                    "[mononoke]: Enable derived data type(s) for {edited} repo(s) (automated backfill reconcile)\n@bypass_size_limit",
                ),
                summary.clone(),
            )
            .prepare(PREPARE_TIMEOUT)
            .await?;

        let test_plan = format!(
            "Automated backfill reconcile. Adds derived data type(s) to the active \
             derived-data config for {edited} repo(s):\n{summary}",
        );
        let diff = mutation.review(reviewers.clone(), test_plan).await?;
        tracing::debug!("created review diff {} for reconcile batch", diff);
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
            cfg.git_delta_manifest_version = Some(GDMV3_VERSION);
        }

        cfg.derivation_batch_sizes
            .get_or_insert_with(BTreeMap::new)
            .entry(p.derived_data_type.name().to_string())
            .or_insert(derivation_batch_size);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use maplit::hashmap;
    use metaconfig_types::DerivedDataTypesConfig;
    use mononoke_macros::mononoke;

    use super::*;

    fn ddc_with(config_name: &str, types: &[DerivableType]) -> DerivedDataConfig {
        DerivedDataConfig {
            enabled_config_name: config_name.to_string(),
            available_configs: hashmap! {
                config_name.to_string() => DerivedDataTypesConfig {
                    types: types.iter().copied().collect(),
                    ..Default::default()
                },
            },
            ..Default::default()
        }
    }

    #[mononoke::test]
    fn pending_when_type_not_in_active_config() {
        let repo_id = RepositoryId::new(1);
        let configs = hashmap! {
            repo_id => ("repo1".to_string(), ddc_with("default", &[DerivableType::Fsnodes])),
        }
        .into_iter()
        .collect();

        let work = compute_work_list(
            vec![(repo_id, DerivableType::GitDeltaManifestsV3)],
            &configs,
        );
        assert_eq!(work.pending.len(), 1);
        assert_eq!(work.pending[0].repo_id, repo_id);
        assert_eq!(work.pending[0].repo_name, "repo1");
        assert_eq!(
            work.pending[0].derived_data_type,
            DerivableType::GitDeltaManifestsV3
        );
        assert_eq!(work.pending[0].enabled_config_name, "default");
    }

    #[mononoke::test]
    fn skipped_when_type_already_in_active_config() {
        let repo_id = RepositoryId::new(1);
        let configs = hashmap! {
            repo_id => (
                "repo1".to_string(),
                ddc_with("default", &[DerivableType::GitDeltaManifestsV3]),
            ),
        }
        .into_iter()
        .collect();

        let work = compute_work_list(
            vec![(repo_id, DerivableType::GitDeltaManifestsV3)],
            &configs,
        );
        assert!(
            work.pending.is_empty(),
            "already-enabled type must not be pending"
        );
        assert_eq!(work.already_in_config, 1);
    }

    #[mononoke::test]
    fn skipped_when_repo_not_in_configs() {
        let configs: BTreeMap<RepositoryId, (String, DerivedDataConfig)> = BTreeMap::new();
        let work = compute_work_list(
            vec![(RepositoryId::new(7), DerivableType::GitDeltaManifestsV3)],
            &configs,
        );
        assert!(
            work.pending.is_empty(),
            "row for unknown repo must be skipped"
        );
        assert_eq!(work.repo_not_found, vec![RepositoryId::new(7)]);
    }

    #[mononoke::test]
    fn pending_when_active_config_name_missing_from_available() {
        // enabled_config_name points at a config not present in available_configs:
        // the type is certainly not enabled there, so it is pending.
        let repo_id = RepositoryId::new(3);
        let mut ddc = ddc_with("default", &[]);
        ddc.enabled_config_name = "nonexistent".to_string();
        let configs = hashmap! { repo_id => ("repo3".to_string(), ddc) }
            .into_iter()
            .collect();

        let work = compute_work_list(vec![(repo_id, DerivableType::Unodes)], &configs);
        assert_eq!(work.pending.len(), 1);
        assert_eq!(work.pending[0].enabled_config_name, "nonexistent");
    }

    #[mononoke::test]
    fn output_is_deterministically_sorted() {
        let r1 = RepositoryId::new(1);
        let r2 = RepositoryId::new(2);
        let configs = hashmap! {
            r1 => ("repo1".to_string(), ddc_with("default", &[])),
            r2 => ("repo2".to_string(), ddc_with("default", &[])),
        }
        .into_iter()
        .collect();

        let work = compute_work_list(
            vec![
                (r2, DerivableType::Unodes),
                (r1, DerivableType::Fsnodes),
                (r1, DerivableType::Unodes),
            ],
            &configs,
        );
        let ordered: Vec<_> = work
            .pending
            .iter()
            .map(|p| (p.repo_id, p.derived_data_type))
            .collect();
        assert_eq!(
            ordered,
            vec![
                (r1, DerivableType::Fsnodes),
                (r1, DerivableType::Unodes),
                (r2, DerivableType::Unodes),
            ],
        );
    }
}
