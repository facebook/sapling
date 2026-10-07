/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
use mononoke_macros::mononoke;
use repo_spec_writer::make_repo_spec_config_path;
use repos::RawRepoConfig;
use repos::RawShardedService;
use repos::RawShardingModeConfig;
use repos::ShardingRegions;
use sql_construct::SqlConstruct;

use super::*;

/// The template this method embeds into every new repo. Pinned here so a
/// rename in repo_spec_writer cannot silently repoint create_repos.
#[mononoke::test]
fn test_template_path_is_the_default_git_repo_spec() {
    assert_eq!(
        repo_spec_writer::DEFAULT_GIT_REPO_SPEC_PATH,
        "scm/mononoke/repos/common/default_git_repo_spec"
    );
}

#[mononoke::test]
fn index_entry_agrees_with_repo_spec() {
    for name in ["org/my-repo", "aosp/platform/vendor/foo"] {
        let request = thrift::RepoCreationRequest {
            repo_name: name.to_string(),
            size_bucket: RepoSizeBucket::LARGE,
            ..Default::default()
        };
        let spec = make_repo_spec(&(RepositoryId::new(7), request), &template()).unwrap();
        let e = RepoIndexEntry::from_repo_spec(&spec).unwrap();
        assert_eq!(
            e.config_path,
            make_repo_spec_config_path(name, RepoSpecDir::Git)
        );
        assert_eq!(e.repo_id, spec.repo_id);
        assert_eq!(e.tiers, spec.tiers);
        assert_eq!(e.enabled, spec.enabled);
        assert_eq!(e.readonly, spec.readonly);
        assert_eq!(e.t_shirt_size, spec.t_shirt_size);
        assert_eq!(e.hipster_acl, spec.hipster_acl);
        assert_eq!(e.enable_git_bundle_uri, spec.enable_git_bundle_uri);
        assert_eq!(
            e.default_commit_identity_scheme,
            spec.default_commit_identity_scheme
        );
        // The fixture's deep_sharding_config has one true status.
        assert!(e.is_deep_sharded);
    }
}

/// Stand-in for `scm/mononoke/repos/common/default_git_repo_spec`. Every
/// value is deliberately one the old hard-coded defaults never produced
/// (tier names, ALL_REGIONS, a tier override, HUGE, bundle_uri=Some),
/// so each assertion tells request-owned from template-owned and a
/// regression to a constant cannot pass by coincidence.
fn template() -> RepoSpec {
    let cfg = RawRepoConfig {
        storage_config: Some("manifold_multiplex_write_packed_wal".to_string()),
        deep_sharding_config: Some(RawShardingModeConfig {
            status: [(RawShardedService::SOURCE_CONTROL_SERVICE, true)]
                .into_iter()
                .collect(),
        }),
        ..Default::default()
    };
    RepoSpec {
        repo_id: 0,
        repo_name: "default/git-template".to_string(),
        hipster_acl: "repos/git/default".to_string(),
        enabled: true,
        readonly: false,
        default_commit_identity_scheme: RawCommitIdentityScheme::GIT,
        enable_git_bundle_uri: Some(false),
        tiers: base(),
        t_shirt_size: TShirtSize::HUGE,
        sharding_regions: ShardingRegions::ALL_REGIONS,
        repo_config: Some(cfg),
        tier_overrides: Some(
            [("tmpl_a".to_string(), RawRepoConfig::default())]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    }
}

/// The fixture template's tier list. Not the production list on purpose:
/// a test passing against these names proves the tiers came from the
/// template, not from a constant.
fn base() -> Vec<String> {
    ["tmpl_a", "tmpl_b"].iter().map(|s| s.to_string()).collect()
}

fn reason(err: scs_errors::ServiceError) -> String {
    match err {
        scs_errors::ServiceError::Internal(e) => e.reason,
        other => panic!("expected an internal error, got {other:?}"),
    }
}

#[mononoke::test]
fn test_validate_template_accepts_the_fixture() {
    validate_template(&template()).expect("the fixture must be a valid template");
    assert_eq!(
        template_storage_name(&template()).unwrap(),
        "manifold_multiplex_write_packed_wal"
    );
}

#[mononoke::test]
fn test_validate_template_rejects_unusable_templates() {
    let no_storage = {
        let mut t = template();
        t.repo_config.as_mut().unwrap().storage_config = None;
        t
    };
    let no_repo_config = RepoSpec {
        repo_config: None,
        ..template()
    };
    let hg = RepoSpec {
        default_commit_identity_scheme: RawCommitIdentityScheme::HG,
        ..template()
    };
    let no_tiers = RepoSpec {
        tiers: vec![],
        ..template()
    };
    let hg_name = format!("{:?}", RawCommitIdentityScheme::HG);
    for (what, bad, expect) in [
        ("missing storage config", no_storage, "storage config"),
        ("repo_config None", no_repo_config, "storage config"),
        ("HG scheme", hg, hg_name.as_str()),
        ("empty tiers", no_tiers, "no tiers"),
    ] {
        let msg = reason(validate_template(&bad).expect_err(what));
        assert!(
            msg.contains(DEFAULT_GIT_REPO_SPEC_PATH),
            "{what}: message must name the template: {msg}"
        );
        assert!(msg.contains(expect), "{what}: {msg}");
    }
}

#[mononoke::test]
fn test_make_repo_spec_copies_template_enabled_false() {
    let disabled = RepoSpec {
        enabled: false,
        ..template()
    };
    let request = thrift::RepoCreationRequest {
        repo_name: "org/my-repo".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        ..Default::default()
    };
    let spec = make_repo_spec(&(RepositoryId::new(1), request), &disabled).unwrap();
    assert!(!spec.enabled, "enabled is template-owned");
}

fn with_multi_repo_land(mut tiers: Vec<String>) -> Vec<String> {
    tiers.push("aosp_multi_repo_land".to_string());
    tiers
}

#[mononoke::test]
fn test_make_repo_spec_file_path_simple_name() {
    // Test asserts the git-only path. When adding HG support to create_repos, add a
    // parallel test for repos/hg/ paths.
    let path = make_repo_spec_file_path("my-repo", RepoSpecDir::Git);
    assert!(
        path.starts_with("source/scm/mononoke/repos/git/"),
        "Path should start with RepoSpec base path: {path}"
    );
    assert!(
        path.ends_with("/my-repo.cconf"),
        "Path should end with /repo-name.cconf: {path}"
    );
}

#[mononoke::test]
fn test_make_repo_spec_file_path_slash_in_name() {
    let path = make_repo_spec_file_path("org/project/repo", RepoSpecDir::Git);
    assert!(
        path.ends_with("/org_project_repo.cconf"),
        "Slashes should be replaced with underscores: {path}"
    );
}

#[mononoke::test]
fn test_make_repo_spec_file_path_no_collision_slash_vs_underscore() {
    let path1 = make_repo_spec_file_path("org/repo", RepoSpecDir::Git);
    let path2 = make_repo_spec_file_path("org_repo", RepoSpecDir::Git);
    assert_ne!(
        path1, path2,
        "Repos differing only in '/' vs '_' must produce different paths"
    );
}

#[mononoke::test]
fn test_make_repo_spec_file_path_deterministic() {
    let path1 = make_repo_spec_file_path("test/repo", RepoSpecDir::Git);
    let path2 = make_repo_spec_file_path("test/repo", RepoSpecDir::Git);
    assert_eq!(path1, path2, "Hash-based path should be deterministic");
}

#[mononoke::test]
fn test_make_repo_spec_file_path_different_repos_may_differ() {
    let path1 = make_repo_spec_file_path("repo-alpha", RepoSpecDir::Git);
    let path2 = make_repo_spec_file_path("repo-beta", RepoSpecDir::Git);
    assert_ne!(
        path1, path2,
        "Different repos should produce different paths"
    );
}

#[mononoke::test]
fn test_make_top_level_acl_name_lowercases_uppercase_org() {
    // Regression for the XF-APAC mirror sync failure on 2026-06-24:
    // before this fix, the uppercase repo name flowed through verbatim
    // to the ACL name on the Mononoke repo config, producing a
    // case-mismatch with the lowercased Hipster ACL.
    assert_eq!(
        make_top_level_acl_name_from_repo_name("XF-APAC/dreamwright-v2"),
        "repos/git/xf-apac",
    );
}

#[mononoke::test]
fn test_make_top_level_acl_name_preserves_already_lowercase_org() {
    // Existing tenants (par-msl) must keep producing the byte-equal
    // ACL name they had before this fix — otherwise their repo
    // configs would point at a different name than the live Hipster
    // entries on the next config rewrite.
    assert_eq!(
        make_top_level_acl_name_from_repo_name("par-msl/risk-test"),
        "repos/git/par-msl",
    );
}

#[mononoke::test]
fn test_make_top_level_acl_name_no_slash() {
    // Defensive: repo name without a slash falls back to using the
    // whole name as the top-level (matches the pre-fix behavior
    // shape), still lowercased.
    assert_eq!(
        make_top_level_acl_name_from_repo_name("Single-Segment"),
        "repos/git/single-segment",
    );
}

#[mononoke::test]
fn test_make_full_acl_name_lowercases() {
    // Custom-ACL path (callers with `custom_acl.is_some()`) also goes
    // through Hipster's lowercasing, so the full ACL name must be
    // lowercased end-to-end.
    assert_eq!(
        make_full_acl_name_from_repo_name("XF-APAC/Dreamwright-V2"),
        "repos/git/xf-apac/dreamwright-v2",
    );
    assert_eq!(
        make_full_acl_name_from_repo_name("par-msl/risk-test"),
        "repos/git/par-msl/risk-test",
    );
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn test_initial_acl_grants_include_coding_crewmates_read() {
    // Every newly-created per-repo Git ACL must grant read to
    // AUTH_SET:coding_crewmates so all Meta engineers can clone the
    // repo. Removing this grant would silently regress the eliminate
    // -per-repo-onboarding-friction commitment made after the
    // provide_gitimport_read_access.sh backfill; grep for that script
    // name before deleting this assertion.
    let grants = initial_acl_grants("some_hipster_group");
    let read = grants
        .iter()
        .find(|g| g.action == "read")
        .expect("initial_acl_grants must contain a read action");
    let has_coding_crewmates = read
        .entry_changes
        .iter()
        .any(|e| e.entry.id_type == AUTH_SET && e.entry.id_data == "coding_crewmates");
    assert!(
        has_coding_crewmates,
        "initial_acl_grants read action must grant AUTH_SET:coding_crewmates",
    );
}

#[cfg(fbcode_build)]
#[mononoke::test]
fn test_initial_acl_grants_include_intern_graphql_controller_read() {
    // Phabricator diff pages read the repo through the intern GraphQL
    // controller with no user identity attached; without this grant every
    // `jf submit` against a fresh repo fails on the metadata read.
    let grants = initial_acl_grants("some_hipster_group");
    let read = grants
        .iter()
        .find(|g| g.action == "read")
        .expect("initial_acl_grants must contain a read action");
    assert!(
        read.entry_changes.iter().any(|e| {
            e.entry.id_type == INTERN_CONTROLLER
                && e.entry.id_data == "XInternGraphGraphQLController"
        }),
        "initial_acl_grants read action must grant INTERN_CONTROLLER:XInternGraphGraphQLController",
    );
}

#[mononoke::test]
fn test_to_repo_spec_tshirt_size_mapping() {
    assert_eq!(
        to_repo_spec_tshirt_size(RepoSizeBucket::EXTRA_SMALL).unwrap(),
        TShirtSize::SMALL
    );
    assert_eq!(
        to_repo_spec_tshirt_size(RepoSizeBucket::SMALL).unwrap(),
        TShirtSize::MEDIUM
    );
    assert_eq!(
        to_repo_spec_tshirt_size(RepoSizeBucket::MEDIUM).unwrap(),
        TShirtSize::MEDIUM
    );
    assert_eq!(
        to_repo_spec_tshirt_size(RepoSizeBucket::LARGE).unwrap(),
        TShirtSize::LARGE
    );
    assert_eq!(
        to_repo_spec_tshirt_size(RepoSizeBucket::EXTRA_LARGE).unwrap(),
        TShirtSize::HUGE
    );
}

#[mononoke::test]
fn test_make_repo_spec_produces_valid_spec() {
    let repo_id = RepositoryId::new(12345);
    let request = thrift::RepoCreationRequest {
        repo_name: "org/my-repo".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        ..Default::default()
    };

    let spec =
        make_repo_spec(&(repo_id, request), &template()).expect("make_repo_spec should succeed");

    assert_eq!(spec.repo_id, 12345);
    assert_eq!(spec.repo_name, "org/my-repo");
    assert!(
        !spec.readonly,
        "readonly defaults to false when not requested"
    );
    assert_eq!(spec.t_shirt_size, TShirtSize::MEDIUM);
    assert_eq!(
        spec.hipster_acl, "repos/git/org",
        "hipster_acl should be the top-level namespace ACL, not the full repo name"
    );
    // Everything else is the template's, verbatim.
    let t = template();
    assert_eq!(spec.enabled, t.enabled);
    assert_eq!(
        spec.default_commit_identity_scheme,
        t.default_commit_identity_scheme
    );
    assert_eq!(spec.sharding_regions, t.sharding_regions);
    assert_eq!(
        spec.tiers, t.tiers,
        "non-aosp repos get the template tiers unchanged"
    );
    assert_eq!(
        spec.repo_config, t.repo_config,
        "new repos carry the template's repo_config, not none"
    );
    assert_eq!(spec.tier_overrides, t.tier_overrides);
}

#[mononoke::test]
fn test_make_repo_spec_request_wins_for_identity_template_for_the_rest() {
    let request = thrift::RepoCreationRequest {
        repo_name: "org/my-repo".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        readonly: Some(true),
        ..Default::default()
    };
    let spec = make_repo_spec(&(RepositoryId::new(12345), request), &template()).unwrap();
    // request-owned
    assert_eq!(spec.repo_id, 12345);
    assert_eq!(spec.repo_name, "org/my-repo");
    assert_eq!(spec.hipster_acl, "repos/git/org");
    assert!(spec.readonly);
    assert_eq!(spec.t_shirt_size, TShirtSize::MEDIUM); // SMALL bucket -> MEDIUM
    // template-owned
    assert!(spec.enabled);
    assert_eq!(
        spec.default_commit_identity_scheme,
        RawCommitIdentityScheme::GIT
    );
    assert_eq!(spec.enable_git_bundle_uri, Some(false));
    assert_eq!(spec.sharding_regions, ShardingRegions::ALL_REGIONS);
    assert_eq!(spec.tiers, base());
    assert_eq!(spec.repo_config, template().repo_config);
    assert_eq!(spec.tier_overrides, template().tier_overrides);
    assert!(spec.tier_overrides.is_some(), "the fixture carries one");
}

#[mononoke::test]
fn test_make_repo_spec_honours_readonly_request() {
    let repo_id = RepositoryId::new(12346);
    let request = thrift::RepoCreationRequest {
        repo_name: "org/mirror-repo".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        readonly: Some(true),
        ..Default::default()
    };

    let spec =
        make_repo_spec(&(repo_id, request), &template()).expect("make_repo_spec should succeed");

    assert!(
        spec.readonly,
        "a request with readonly=true must produce a read-only RepoSpec"
    );
}

#[mononoke::test]
fn test_make_repo_spec_uses_top_level_acl_for_aosp_repo() {
    let repo_id = RepositoryId::new(18279);
    let request = thrift::RepoCreationRequest {
        repo_name: "aosp/platform/vendor/meta/prebuilts/assets".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        ..Default::default()
    };

    let spec =
        make_repo_spec(&(repo_id, request), &template()).expect("make_repo_spec should succeed");

    assert_eq!(
        spec.hipster_acl, "repos/git/aosp",
        "AOSP repos must use the top-level `repos/git/aosp` ACL, not a non-existent full-path ACL"
    );
}

#[mononoke::test]
fn test_make_repo_spec_uses_full_name_when_no_slash() {
    let repo_id = RepositoryId::new(99999);
    let request = thrift::RepoCreationRequest {
        repo_name: "simple-repo".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        ..Default::default()
    };

    let spec =
        make_repo_spec(&(repo_id, request), &template()).expect("make_repo_spec should succeed");

    assert_eq!(
        spec.hipster_acl, "repos/git/simple-repo",
        "Repos without `/` should use the full name as the ACL"
    );
}

#[mononoke::test]
fn test_tier_list_for_repo_spec_aosp_prefix_adds_multi_repo_land() {
    assert_eq!(
        tier_list_for_repo_spec(&base(), "aosp/platform/vendor/foo"),
        with_multi_repo_land(base()),
        "aosp/* repos must include aosp_multi_repo_land tier"
    );
}

#[mononoke::test]
fn test_tier_list_for_repo_spec_nested_aosp_adds_multi_repo_land() {
    // Substring match: repos with `aosp/` deeper in the path (e.g. the
    // Oculus AOSP fork) must also be on the aosp_multi_repo_land tier.
    assert_eq!(
        tier_list_for_repo_spec(&base(), "oculus/aosp/vendor/oculus"),
        with_multi_repo_land(base()),
        "repos containing aosp/ as a substring must include aosp_multi_repo_land tier"
    );
}

#[mononoke::test]
fn test_tier_list_for_repo_spec_non_aosp_excluded_from_multi_repo_land() {
    assert_eq!(
        tier_list_for_repo_spec(&base(), "manus/foo"),
        base(),
        "non-aosp repos must NOT include aosp_multi_repo_land tier"
    );
    assert_eq!(
        tier_list_for_repo_spec(&base(), "simple-repo"),
        base(),
        "simple repos must NOT include aosp_multi_repo_land tier"
    );
    // Boundary: a repo literally named "aosp" (no slash) does NOT contain `aosp/`.
    assert_eq!(
        tier_list_for_repo_spec(&base(), "aosp"),
        base(),
        "literal name 'aosp' (no trailing /) must NOT match the aosp/ substring"
    );
    // Boundary: confusingly-named prefix that shares "aosp" but isn't `aosp/`.
    assert_eq!(
        tier_list_for_repo_spec(&base(), "aosp_extras/foo"),
        base(),
        "aosp_extras/* must NOT match the aosp/ substring"
    );
}

#[mononoke::test]
fn test_make_repo_spec_aosp_appends_multi_repo_land_to_template_tiers() {
    let request = thrift::RepoCreationRequest {
        repo_name: "aosp/platform/vendor/meta/prebuilts/assets".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        ..Default::default()
    };
    let spec = make_repo_spec(&(RepositoryId::new(18279), request), &template()).unwrap();
    assert_eq!(
        spec.tiers,
        ["tmpl_a", "tmpl_b", "aosp_multi_repo_land"],
        "AOSP repos get the template tiers plus aosp_multi_repo_land so multi_repo_land_service can serve them"
    );
}

#[mononoke::test]
fn test_default_branch_validation() {
    for invalid in [
        "",
        "HEAD",
        "head",
        "Head",
        "hEaD",
        "refs/heads/main",
        "refs/tags/v1.0",
    ] {
        let err = validate_default_branch(invalid)
            .expect_err(&format!("'{invalid}' must be rejected as a default_branch"));
        assert!(
            matches!(err, scs_errors::ServiceError::Request(_)),
            "'{invalid}' must be rejected as an invalid request, got: {err:?}"
        );
    }
    for valid in ["main", "release/1.0"] {
        assert_eq!(
            validate_default_branch(valid)
                .unwrap_or_else(|err| panic!("'{valid}' must be accepted, got: {err:?}")),
            valid,
        );
    }
}

#[mononoke::fbinit_test]
async fn test_write_default_branch_symref_writes_head_row(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(RepositoryId::new(1));

    write_default_branch_symref(&ctx, &store, "main")
        .await
        .expect("writing the HEAD symref should succeed");

    let entry = store
        .get_ref_by_symref(&ctx, "HEAD".to_string())
        .await?
        .expect("a HEAD symref row should exist after the write");
    assert_eq!(entry.ref_name, "main");
    assert_eq!(entry.ref_name_with_type(), "refs/heads/main");
    Ok(())
}

#[mononoke::test]
fn test_make_repo_spec_uses_full_acl_when_custom_acl_set() {
    let repo_id = RepositoryId::new(55555);
    let request = thrift::RepoCreationRequest {
        repo_name: "fairinternal/occhi".to_string(),
        size_bucket: RepoSizeBucket::SMALL,
        custom_acl: Some(thrift::CustomAclParams {
            hipster_group: "oncall_onevision".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let spec =
        make_repo_spec(&(repo_id, request), &template()).expect("make_repo_spec should succeed");

    assert_eq!(
        spec.hipster_acl, "repos/git/fairinternal/occhi",
        "Repos with custom_acl should use the full-path ACL"
    );
}
