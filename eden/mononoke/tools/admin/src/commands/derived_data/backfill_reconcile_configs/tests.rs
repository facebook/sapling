/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

/// A git repo's reconcile info (the common case in tests).
fn info(repo_name: &str, config_name: &str, types: &[DerivableType]) -> RepoReconcileInfo {
    RepoReconcileInfo {
        repo_name: repo_name.to_string(),
        commit_identity_scheme: CommitIdentityScheme::GIT,
        derived_data_config: ddc_with(config_name, types),
    }
}

#[mononoke::test]
fn pending_when_type_not_in_active_config() {
    let repo_id = RepositoryId::new(1);
    let configs = hashmap! {
        repo_id => info("repo1", "default", &[DerivableType::ContentManifests]),
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
fn hg_repo_carries_its_hg_identity_scheme() {
    // Regression test: reconcile used to build every path via the git-only
    // `make_repo_spec_file_path`, so an hg repo's edit was aimed at
    // `repos/git/e0/scs-configerator_test.cconf` — which does not exist, and
    // the whole batch failed with "No config entry found". The scheme has to
    // survive into PendingReconcile for `repo_spec_dir_for` to pick repos/hg/.
    let repo_id = RepositoryId::new(403);
    let configs = hashmap! {
        repo_id => RepoReconcileInfo {
            repo_name: "scs-configerator_test".to_string(),
            commit_identity_scheme: CommitIdentityScheme::HG,
            derived_data_config: ddc_with("default", &[DerivableType::ContentManifests]),
        },
    }
    .into_iter()
    .collect();

    let work = compute_work_list(
        vec![(repo_id, DerivableType::SkeletonManifestsV2)],
        &configs,
    );
    assert_eq!(work.pending.len(), 1);
    assert_eq!(
        work.pending[0].commit_identity_scheme,
        CommitIdentityScheme::HG,
        "hg repo must not be reconciled as if it were a git repo",
    );
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn repo_spec_dir_follows_commit_identity_scheme() {
    use repo_spec_writer::RepoSpecDir;
    use repo_spec_writer::make_repo_spec_file_path;

    let pending = |scheme| super::PendingReconcile {
        repo_id: RepositoryId::new(403),
        repo_name: "scs-configerator_test".to_string(),
        derived_data_type: DerivableType::ContentManifests,
        enabled_config_name: "default".to_string(),
        commit_identity_scheme: scheme,
    };

    let hg = pending(CommitIdentityScheme::HG);
    let hg_dir = super::fb::repo_spec_dir_for(&hg).unwrap();
    assert_eq!(hg_dir, RepoSpecDir::Hg);
    assert_eq!(
        make_repo_spec_file_path(&hg.repo_name, hg_dir),
        "source/scm/mononoke/repos/hg/e0/scs-configerator_test.cconf",
    );

    let git = pending(CommitIdentityScheme::GIT);
    assert_eq!(
        super::fb::repo_spec_dir_for(&git).unwrap(),
        RepoSpecDir::Git
    );

    // No RepoSpec tree exists for these, so guessing would target the wrong file.
    for scheme in [CommitIdentityScheme::BONSAI, CommitIdentityScheme::UNKNOWN] {
        assert!(
            super::fb::repo_spec_dir_for(&pending(scheme.clone())).is_err(),
            "{scheme:?} must not resolve to a RepoSpec directory",
        );
    }
}

#[mononoke::test]
fn skipped_when_type_already_in_active_config() {
    let repo_id = RepositoryId::new(1);
    let configs = hashmap! {
        repo_id => info("repo1", "default", &[DerivableType::GitDeltaManifestsV3]),
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
    let configs: BTreeMap<RepositoryId, RepoReconcileInfo> = BTreeMap::new();
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
    let mut repo3 = info("repo3", "default", &[]);
    repo3.derived_data_config.enabled_config_name = "nonexistent".to_string();
    let configs = hashmap! { repo_id => repo3 }.into_iter().collect();

    let work = compute_work_list(vec![(repo_id, DerivableType::Unodes)], &configs);
    assert_eq!(work.pending.len(), 1);
    assert_eq!(work.pending[0].enabled_config_name, "nonexistent");
}

#[mononoke::test]
fn output_is_deterministically_sorted() {
    let r1 = RepositoryId::new(1);
    let r2 = RepositoryId::new(2);
    let configs = hashmap! {
        r1 => info("repo1", "default", &[]),
        r2 => info("repo2", "default", &[]),
    }
    .into_iter()
    .collect();

    let work = compute_work_list(
        vec![
            (r2, DerivableType::Unodes),
            (r1, DerivableType::ContentManifests),
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
            (r1, DerivableType::ContentManifests),
            (r1, DerivableType::Unodes),
            (r2, DerivableType::Unodes),
        ],
    );
}

fn tmpl(types: &[DerivableType], gdm_version: Option<i16>) -> TemplateDriftInput {
    TemplateDriftInput {
        enabled_config_name: "default".to_string(),
        types: types.iter().copied().collect(),
        git_delta_manifest_version: gdm_version,
    }
}

/// An hg repo's reconcile info.
fn hg_info(repo_name: &str, types: &[DerivableType]) -> RepoReconcileInfo {
    RepoReconcileInfo {
        commit_identity_scheme: CommitIdentityScheme::HG,
        ..info(repo_name, "default", types)
    }
}

#[mononoke::test]
fn gdm_v2_is_superseded_only_above_version_2() {
    for (v, expect) in [
        (None, false),
        (Some(2), false),
        (Some(3), true),
        (Some(4), true),
    ] {
        assert_eq!(
            superseded_in_template(DerivableType::GitDeltaManifestsV2, &tmpl(&[], v)),
            expect,
            "{v:?}"
        );
    }
    assert!(!superseded_in_template(
        DerivableType::FastlogV2,
        &tmpl(&[], Some(3))
    ));
}

#[mononoke::test]
fn template_drift_reports_types_at_or_above_half_of_git_repos() {
    let configs: BTreeMap<RepositoryId, RepoReconcileInfo> = [
        (
            1,
            info(
                "a",
                "default",
                &[DerivableType::Fsnodes, DerivableType::FastlogV2],
            ),
        ),
        (
            2,
            info(
                "b",
                "default",
                &[DerivableType::Fsnodes, DerivableType::FastlogV2],
            ),
        ),
        (
            3,
            info(
                "c",
                "default",
                &[DerivableType::Fsnodes, DerivableType::Unodes],
            ),
        ),
        (
            4,
            info("d", "content_manifests_rollout", &[DerivableType::Fsnodes]),
        ),
    ]
    .into_iter()
    .map(|(i, v)| (RepositoryId::new(i), v))
    .collect();
    let d = template_drift(&configs, &tmpl(&[DerivableType::Fsnodes], Some(3)));
    assert_eq!(d.git_total, 4);
    // FastlogV2 is on 2 of 4 (exactly half): reported. Unodes is on 1 of 4: not.
    // Fsnodes is on all 4 but already on the template: not.
    assert_eq!(d.lacking, vec![(DerivableType::FastlogV2, 2)]);
}

#[mononoke::test]
fn template_drift_ignores_hg_repos_and_superseded_types() {
    let configs: BTreeMap<RepositoryId, RepoReconcileInfo> = [
        (
            1,
            info("a", "default", &[DerivableType::GitDeltaManifestsV2]),
        ),
        (2, hg_info("fbsource", &[DerivableType::Unodes])),
    ]
    .into_iter()
    .map(|(i, v)| (RepositoryId::new(i), v))
    .collect();
    assert!(
        template_drift(&configs, &tmpl(&[], Some(3)))
            .lacking
            .is_empty()
    );
    assert_eq!(
        template_drift(&configs, &tmpl(&[], Some(2))).lacking,
        vec![(DerivableType::GitDeltaManifestsV2, 1)]
    );
    assert_eq!(template_drift(&configs, &tmpl(&[], None)).git_total, 1);
}

#[mononoke::test]
fn template_drift_skips_repos_whose_enabled_config_is_missing() {
    let mut broken = info("a", "default", &[DerivableType::Unodes]);
    broken.derived_data_config.enabled_config_name = "nonexistent".to_string();
    let configs: BTreeMap<RepositoryId, RepoReconcileInfo> =
        [(RepositoryId::new(1), broken)].into_iter().collect();
    let d = template_drift(&configs, &tmpl(&[], None));
    assert_eq!(d.git_total, 1);
    assert!(d.lacking.is_empty());
}

#[mononoke::test]
fn plan_template_edits_classifies_add_present_and_refuses_superseded() {
    let t = tmpl(&[DerivableType::Fsnodes], Some(3));
    let plan =
        plan_template_edits(&t, &[DerivableType::FastlogV2, DerivableType::Fsnodes]).unwrap();
    assert_eq!(
        plan,
        vec![
            TemplatePlanItem::Add(DerivableType::FastlogV2),
            TemplatePlanItem::AlreadyPresent(DerivableType::Fsnodes),
        ]
    );
    let err = plan_template_edits(&t, &[DerivableType::GitDeltaManifestsV2]).unwrap_err();
    assert!(err.to_string().contains("superseded"), "{err}");
    assert!(plan_template_edits(&t, &[]).unwrap().is_empty());
    let one = plan_template_edits(&t, &[DerivableType::FastlogV2]).unwrap();
    assert_eq!(one, vec![TemplatePlanItem::Add(DerivableType::FastlogV2)]);
    let three = plan_template_edits(
        &t,
        &[
            DerivableType::FastlogV2,
            DerivableType::Unodes,
            DerivableType::Fsnodes,
        ],
    )
    .unwrap();
    assert_eq!(
        three,
        vec![
            TemplatePlanItem::Add(DerivableType::FastlogV2),
            TemplatePlanItem::Add(DerivableType::Unodes),
            TemplatePlanItem::AlreadyPresent(DerivableType::Fsnodes),
        ]
    );
}

#[mononoke::test]
fn template_review_texts_name_the_types_and_have_no_size_bypass() {
    let title = template_review_diff_title(&[DerivableType::FastlogV2]);
    assert_eq!(
        title,
        "[mononoke]: Enable `fastlog_v2` for new Git repos (create_repos template)"
    );
    assert!(!title.contains("bypass_size_limit"));
    let three = [
        DerivableType::FastlogV2,
        DerivableType::Unodes,
        DerivableType::Fsnodes,
    ];
    let summary = template_review_diff_summary(&three, "default");
    assert!(summary.contains("scm/mononoke/repos/common/default_git_repo_spec"));
    assert!(summary.contains("`default`"));
    for t in three {
        assert!(summary.contains(t.name()));
    }
    assert!(template_review_diff_test_plan().contains("repo_spec.ctest"));
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(flatten)]
    args: BackfillReconcileConfigsArgs,
}

#[mononoke::test]
fn enable_for_new_repos_parses_documented_names_only() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "backfill-reconcile-configs",
        "--enable-for-new-repos",
        "fastlog_v2,git_delta_manifests_v2",
    ])
    .unwrap();
    assert_eq!(
        cli.args.enable_for_new_repos,
        vec![DerivableType::FastlogV2, DerivableType::GitDeltaManifestsV2],
    );
    let err = match Cli::try_parse_from([
        "backfill-reconcile-configs",
        "--enable-for-new-repos",
        "FastlogV2",
    ]) {
        Ok(_) => panic!("a variant name is not a documented type name"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation, "{err}");
    assert!(
        Cli::try_parse_from(["backfill-reconcile-configs"])
            .unwrap()
            .args
            .enable_for_new_repos
            .is_empty()
    );
}

#[mononoke::test]
fn template_pending_reconcile_uses_the_templates_own_enabled_config() {
    for name in ["content_manifests_rollout", "default"] {
        let p =
            template_pending_reconcile(0, "default/git-template", name, DerivableType::FastlogV2);
        assert_eq!(p.repo_id, RepositoryId::new(0));
        assert_eq!(p.enabled_config_name, name);
        assert_eq!(p.commit_identity_scheme, CommitIdentityScheme::GIT);
    }
}

// `reduce_template` lives in `mod fb`; `#[cfg(fbcode_build)]` inside
// `#[cfg(test)] mod tests` is `all(test, fbcode_build)`. Same form as
// `repo_spec_dir_follows_commit_identity_scheme`.
#[cfg(fbcode_build)]
#[mononoke::test]
fn reduce_template_maps_known_names_and_skips_unknown() {
    use repos::RawDerivedDataConfig;
    use repos::RawDerivedDataTypesConfig;
    use repos::RawRepoConfig;
    use repos::RepoSpec;
    let spec = RepoSpec {
        repo_config: Some(RawRepoConfig {
            derived_data_config: Some(RawDerivedDataConfig {
                enabled_config_name: Some("default".to_string()),
                available_configs: Some(
                    std::iter::once((
                        "default".to_string(),
                        RawDerivedDataTypesConfig {
                            types: ["unodes", "bogus"].iter().map(|s| s.to_string()).collect(),
                            git_delta_manifest_version: Some(3),
                            ..Default::default()
                        },
                    ))
                    .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let t = super::fb::reduce_template(&spec).unwrap();
    assert_eq!(
        t.types,
        [DerivableType::Unodes].into_iter().collect::<BTreeSet<_>>()
    );
    assert_eq!(t.git_delta_manifest_version, Some(3));
    assert_eq!(t.enabled_config_name, "default");
    let mut broken = spec.clone();
    broken
        .repo_config
        .as_mut()
        .unwrap()
        .derived_data_config
        .as_mut()
        .unwrap()
        .enabled_config_name = Some("nope".to_string());
    assert!(super::fb::reduce_template(&broken).is_err());
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn template_path_matches_repo_spec_writer() {
    assert_eq!(
        DEFAULT_GIT_REPO_SPEC_PATH_STR,
        repo_spec_writer::DEFAULT_GIT_REPO_SPEC_PATH
    );
}

#[mononoke::test]
fn plan_template_edits_dedupes_repeated_names() {
    let t = tmpl(&[DerivableType::Fsnodes], Some(3));
    let plan = plan_template_edits(
        &t,
        &[
            DerivableType::FastlogV2,
            DerivableType::FastlogV2,
            DerivableType::Fsnodes,
            DerivableType::Fsnodes,
        ],
    )
    .unwrap();
    assert_eq!(
        plan,
        vec![
            TemplatePlanItem::Add(DerivableType::FastlogV2),
            TemplatePlanItem::AlreadyPresent(DerivableType::Fsnodes),
        ]
    );
}
