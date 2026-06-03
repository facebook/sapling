/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use borrowed::borrowed;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_rate_limit_config::CommitRateLimit;
use commit_rate_limit_config::CommitRateLimitCacheConfig;
use commit_rate_limit_config::CommitRateLimitRule;
use commit_rate_limit_config::EligibilityCheck;
use commit_rate_limit_config::EligibleChangesetInfo;
use commit_rate_limit_config::RateLimit;
use commit_rate_limit_config::cache::ChangesetEligibilityCache;
use commit_rate_limit_config::inspect_changeset_eligibility;
use commit_rate_limit_config::is_eligible_for_rate_limit;
use commit_rate_limit_config::matches_user_filter;
use commit_rate_limit_config::parse_author_username;
use commit_rate_limit_config::touches_directories;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;
use tests_utils::bookmark;

use super::*;

// --- Test repo type ---

#[facet::container]
#[derive(Clone)]
struct TestRepo {
    #[facet]
    bookmarks: dyn bookmarks::Bookmarks,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    commit_rate_limit: CommitRateLimit,
}

// =========================================================================
// Constants and helpers
// =========================================================================

const ALICE: &str = "Alice <alice@fb.com>";
const BOB: &str = "Bob <bob@fb.com>";

/// The commit message tag used by all tests.
const ELIGIBLE_TAG: &str = "AUTO_APPROVED";

/// The hg_extra key used by tests (for the setup DAG).
const ELIGIBLE_EXTRA: &str = "auto_approved";

fn recent_date() -> DateTime {
    DateTime::now()
}

fn make_config(directories: &[&str], per_user: bool, max_commits: u64) -> CommitRateLimitRule {
    let dirs = directories.iter().map(|d| d.to_string()).collect();
    CommitRateLimitRule::new(
        "test_hook".to_string(),
        "test_repo".to_string(),
        vec![
            EligibilityCheck::CommitMessageTag {
                tag: ELIGIBLE_TAG.to_string(),
            },
            EligibilityCheck::HgExtra {
                key: ELIGIBLE_EXTRA.to_string(),
            },
        ],
        vec![RateLimit::new(3600, max_commits).expect("valid rate limit")],
        dirs,
        per_user,
        None,
    )
}

fn make_changeset(extras: Vec<(&str, &[u8])>, files: Vec<&str>, message: &str) -> BonsaiChangeset {
    let hg_extra = extras
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_vec()))
        .collect();
    let file_changes = files
        .into_iter()
        .enumerate()
        .map(|(i, path)| {
            let mut content_id_bytes = [0u8; 32];
            content_id_bytes[0] = (i + 1) as u8;
            (
                NonRootMPath::new(path).expect("test path must be valid"),
                FileChange::tracked(
                    ContentId::from_bytes(content_id_bytes).expect("failed to load content id"),
                    FileType::Regular,
                    10,
                    None,
                    GitLfs::FullContent,
                ),
            )
        })
        .collect();
    BonsaiChangesetMut {
        author: "Test User <testuser@fb.com>".to_string(),
        message: message.to_string(),
        hg_extra,
        file_changes,
        ..Default::default()
    }
    .freeze()
    .expect("test changeset must be valid")
}

fn make_changeset_with_extras_and_files(
    extras: Vec<(&str, &[u8])>,
    files: Vec<&str>,
) -> BonsaiChangeset {
    make_changeset(extras, files, "message")
}

// =========================================================================
// Unit tests: RateLimit validation
// =========================================================================

#[mononoke::test]
fn test_rate_limit_valid_window() {
    assert!(RateLimit::new(3600, 10).is_ok());
}

#[mononoke::test]
fn test_rate_limit_max_window() {
    assert!(RateLimit::new(6 * 60 * 60, 10).is_ok());
}

#[mononoke::test]
fn test_rate_limit_window_too_large() {
    let result = RateLimit::new(7 * 60 * 60, 10);
    assert!(result.is_err());
    let err_msg = result.expect_err("expected error").to_string();
    assert!(
        err_msg.contains("exceeds maximum"),
        "Expected 'exceeds maximum' in error: {}",
        err_msg
    );
}

#[mononoke::test]
fn test_rate_limit_zero_window_rejected() {
    assert!(RateLimit::new(0, 10).is_err());
}

#[mononoke::test]
fn test_rate_limit_zero_max_commits_rejected() {
    assert!(RateLimit::new(3600, 0).is_err());
}

// =========================================================================
// Unit tests: CommitMessageTag eligibility
// =========================================================================

#[mononoke::test]
fn test_commit_message_tag_eligible() {
    let cs = make_changeset(vec![], vec!["a.txt"], "fix: apply AUTO_APPROVED changes");
    let check = EligibilityCheck::CommitMessageTag {
        tag: "AUTO_APPROVED".to_string(),
    };
    assert!(check.is_eligible(&cs));
}

#[mononoke::test]
fn test_commit_message_tag_not_present() {
    let cs = make_changeset(vec![], vec!["a.txt"], "regular commit message");
    let check = EligibilityCheck::CommitMessageTag {
        tag: "AUTO_APPROVED".to_string(),
    };
    assert!(!check.is_eligible(&cs));
}

#[mononoke::test]
fn test_commit_message_tag_case_sensitive() {
    let cs = make_changeset(vec![], vec!["a.txt"], "auto_approved lowercase");
    let check = EligibilityCheck::CommitMessageTag {
        tag: "AUTO_APPROVED".to_string(),
    };
    assert!(!check.is_eligible(&cs));
}

#[mononoke::test]
fn test_commit_message_tag_partial_match() {
    let cs = make_changeset(vec![], vec!["a.txt"], "this is AUTO_APPROVED_V2 stuff");
    let check = EligibilityCheck::CommitMessageTag {
        tag: "AUTO_APPROVED".to_string(),
    };
    // Substring match -- "AUTO_APPROVED" is contained in "AUTO_APPROVED_V2"
    assert!(check.is_eligible(&cs));
}

#[mononoke::test]
fn test_commit_message_tag_empty_message() {
    let cs = make_changeset(vec![], vec!["a.txt"], "");
    let check = EligibilityCheck::CommitMessageTag {
        tag: "AUTO_APPROVED".to_string(),
    };
    assert!(!check.is_eligible(&cs));
}

// =========================================================================
// Unit tests: HgExtra eligibility
// =========================================================================

#[mononoke::test]
fn test_hg_extra_eligible() {
    let cs = make_changeset_with_extras_and_files(vec![("auto_approved", b"1")], vec!["a.txt"]);
    let check = EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    };
    assert!(check.is_eligible(&cs));
}

#[mononoke::test]
fn test_hg_extra_not_present() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["a.txt"]);
    let check = EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    };
    assert!(!check.is_eligible(&cs));
}

#[mononoke::test]
fn test_hg_extra_wrong_key() {
    let cs = make_changeset_with_extras_and_files(vec![("other_key", b"1")], vec!["a.txt"]);
    let check = EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    };
    assert!(!check.is_eligible(&cs));
}

// =========================================================================
// Unit tests: OR semantics across eligibility checks
// =========================================================================

#[mononoke::test]
fn test_or_semantics_message_tag_matches() {
    let cs = make_changeset(vec![], vec!["a.txt"], "commit AUTO_APPROVED");
    let checks = vec![
        EligibilityCheck::CommitMessageTag {
            tag: "AUTO_APPROVED".to_string(),
        },
        EligibilityCheck::HgExtra {
            key: "auto_approved".to_string(),
        },
    ];
    assert!(is_eligible_for_rate_limit(&checks, &cs));
}

#[mononoke::test]
fn test_or_semantics_hg_extra_matches() {
    let cs = make_changeset_with_extras_and_files(vec![("auto_approved", b"1")], vec!["a.txt"]);
    let checks = vec![
        EligibilityCheck::CommitMessageTag {
            tag: "AUTO_APPROVED".to_string(),
        },
        EligibilityCheck::HgExtra {
            key: "auto_approved".to_string(),
        },
    ];
    assert!(is_eligible_for_rate_limit(&checks, &cs));
}

#[mononoke::test]
fn test_or_semantics_neither_matches() {
    let cs = make_changeset(vec![], vec!["a.txt"], "plain commit");
    let checks = vec![
        EligibilityCheck::CommitMessageTag {
            tag: "AUTO_APPROVED".to_string(),
        },
        EligibilityCheck::HgExtra {
            key: "auto_approved".to_string(),
        },
    ];
    assert!(!is_eligible_for_rate_limit(&checks, &cs));
}

// =========================================================================
// Unit tests: Directory filter
// =========================================================================

#[mononoke::test]
fn test_touches_directories_match() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["users/alice/foo.txt"]);
    let dirs = vec!["users/".to_string()];
    assert!(touches_directories(&cs, &dirs));
}

#[mononoke::test]
fn test_touches_directories_no_match() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["fbcode/bar.txt"]);
    let dirs = vec!["users/".to_string()];
    assert!(!touches_directories(&cs, &dirs));
}

#[mononoke::test]
fn test_empty_directories_always_matches() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["anything/file.txt"]);
    let dirs: Vec<String> = vec![];
    assert!(touches_directories(&cs, &dirs));
}

#[mononoke::test]
fn test_touches_directories_multiple_prefixes() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["configs/settings.json"]);
    let dirs = vec!["users/".to_string(), "configs/".to_string()];
    assert!(touches_directories(&cs, &dirs));
}

// =========================================================================
// Unit tests: Predicate
// =========================================================================

#[mononoke::test]
fn test_build_ancestor_predicate_eligible_and_touches() {
    let cs = make_changeset(
        vec![("auto_approved", b"1")],
        vec!["users/alice/foo.txt"],
        "msg",
    );
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let predicate = build_ancestor_predicate(&checks, &dirs, None, true);
    assert!(predicate(&cs));
}

#[mononoke::test]
fn test_build_ancestor_predicate_not_eligible() {
    let cs = make_changeset_with_extras_and_files(vec![], vec!["users/alice/foo.txt"]);
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let predicate = build_ancestor_predicate(&checks, &dirs, None, true);
    assert!(!predicate(&cs));
}

#[mononoke::test]
fn test_build_ancestor_predicate_wrong_directory() {
    let cs = make_changeset(vec![("auto_approved", b"1")], vec!["other.txt"], "msg");
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let predicate = build_ancestor_predicate(&checks, &dirs, None, true);
    assert!(!predicate(&cs));
}

#[mononoke::test]
fn test_build_ancestor_predicate_user_filter_match() {
    let cs = make_changeset(
        vec![("auto_approved", b"1")],
        vec!["users/alice/foo.txt"],
        "msg",
    );
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let predicate = build_ancestor_predicate(&checks, &dirs, Some("testuser"), true);
    assert!(predicate(&cs));
}

#[mononoke::test]
fn test_build_ancestor_predicate_user_filter_no_match() {
    let cs = make_changeset(
        vec![("auto_approved", b"1")],
        vec!["users/alice/foo.txt"],
        "msg",
    );
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let predicate = build_ancestor_predicate(&checks, &dirs, Some("otheruser"), true);
    assert!(!predicate(&cs));
}

// =========================================================================
// Unit tests: Config-stable inspection and user filter
// =========================================================================

#[mononoke::test]
fn test_inspect_eligible_changeset() {
    let cs = make_changeset(
        vec![("auto_approved", b"1")],
        vec!["users/alice/foo.txt"],
        "msg",
    );
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let info = inspect_changeset_eligibility(&cs, &checks, &dirs, true);
    assert!(info.is_some());
    assert_eq!(
        info.as_ref().and_then(|i| i.parsed_username.as_deref()),
        Some("testuser")
    );
}

#[mononoke::test]
fn test_inspect_ineligible_changeset() {
    let cs = make_changeset(vec![], vec!["users/alice/foo.txt"], "msg");
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let info = inspect_changeset_eligibility(&cs, &checks, &dirs, true);
    assert!(info.is_none());
}

#[mononoke::test]
fn test_inspect_wrong_directory() {
    let cs = make_changeset(vec![("auto_approved", b"1")], vec!["other.txt"], "msg");
    let checks = vec![EligibilityCheck::HgExtra {
        key: "auto_approved".to_string(),
    }];
    let dirs = vec!["users/".to_string()];
    let info = inspect_changeset_eligibility(&cs, &checks, &dirs, true);
    assert!(info.is_none());
}

#[mononoke::test]
fn test_matches_user_filter_no_filter() {
    let info = EligibleChangesetInfo {
        parsed_username: Some("alice".to_string()),
    };
    assert!(matches_user_filter(&info, None));
}

#[mononoke::test]
fn test_matches_user_filter_match() {
    let info = EligibleChangesetInfo {
        parsed_username: Some("alice".to_string()),
    };
    assert!(matches_user_filter(&info, Some("alice")));
}

#[mononoke::test]
fn test_matches_user_filter_no_match() {
    let info = EligibleChangesetInfo {
        parsed_username: Some("alice".to_string()),
    };
    assert!(!matches_user_filter(&info, Some("bob")));
}

#[mononoke::test]
fn test_matches_user_filter_no_username() {
    let info = EligibleChangesetInfo {
        parsed_username: None,
    };
    assert!(!matches_user_filter(&info, Some("alice")));
}

// =========================================================================
// Integration tests
//
// These exercise the complete check_commit_rate_limit() path with real
// commits in a test repo. They verify the restriction hierarchy of 4
// config instances:
//
//   (1) Global:          all dirs, all users, max=10
//   (2) Per-user:        all dirs, per user,  max=6
//   (3) users/ global:   users/ dir, all users, max=5
//   (4) users/ per-user: users/ dir, per user,  max=4
//
// Restriction increases (1)->(4): users/ per-user is most restrictive.
// =========================================================================

/// Create the shared test DAG with eligible ancestors.
///
/// Public history (bookmark "main" points at `tip`):
///   R -> A(eligible,users/,alice) -> B(eligible,users/,alice)
///     -> C(eligible,fbcode/,bob) -> D(eligible,users/,bob)
///     -> E(eligible,users/,alice) -> F(eligible,fbcode/,alice)
///     -> G(eligible,users/,alice) -> tip(not eligible, users/, alice)
///
/// Eligible ancestor counts from `tip`:
///   All eligible:          A,B,C,D,E,F,G = 7
///   users/ eligible:       A,B,D,E,G     = 5
///   alice all eligible:    A,B,E,F,G     = 5
///   alice users/ eligible: A,B,E,G       = 4
async fn setup_test_repo(
    fb: FacebookInit,
) -> Result<(CoreContext, TestRepo, BookmarkKey, ChangesetId)> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("README", "init")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let a = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("users/alice/a.txt", "a")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let b = CreateCommitContext::new(&ctx, &repo, vec![a])
        .add_file("users/alice/b.txt", "b")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let c = CreateCommitContext::new(&ctx, &repo, vec![b])
        .add_file("fbcode/server/c.txt", "c")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(BOB)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let d = CreateCommitContext::new(&ctx, &repo, vec![c])
        .add_file("users/bob/d.txt", "d")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(BOB)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let e = CreateCommitContext::new(&ctx, &repo, vec![d])
        .add_file("users/alice/e.txt", "e")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let f = CreateCommitContext::new(&ctx, &repo, vec![e])
        .add_file("fbcode/server/f.txt", "f")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let g = CreateCommitContext::new(&ctx, &repo, vec![f])
        .add_file("users/alice/g.txt", "g")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let tip = CreateCommitContext::new(&ctx, &repo, vec![g])
        .add_file("users/alice/tip.txt", "tip")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let bm = bookmark(&ctx, &repo, "main").create_publishing(tip).await?;

    Ok((ctx, repo, bm, tip))
}

/// Non-eligible commit is NEVER blocked, even when at the limit.
/// This is the most critical safety property.
#[mononoke::fbinit_test]
async fn test_non_eligible_commit_never_blocked(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 10);
    let per_user = make_config(&[], true, 6);
    let users_global = make_config(&["users/"], false, 5);
    let users_per_user = make_config(&["users/"], true, 4);

    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/draft.txt", "draft")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // The draft is not eligible (no AUTO_APPROVED tag / auto_approved extra),
    // so every rule skips it -- and a skipped rule never blocks the commit.
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Skipped,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &per_user, Some("alice"), true).await?,
        RateLimitOutcome::Skipped,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_global, None, true).await?,
        RateLimitOutcome::Skipped,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Skipped,
    );
    Ok(())
}

/// Eligible commit under all limits: all configs accept.
#[mononoke::fbinit_test]
async fn test_under_all_limits_accepted(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 100);
    let per_user = make_config(&[], true, 100);
    let users_global = make_config(&["users/"], false, 100);
    let users_per_user = make_config(&["users/"], true, 100);

    // Use commit message tag for eligibility (primary production path)
    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/draft.txt", "draft")
        .set_message(format!("eligible commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &per_user, Some("alice"), true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_global, None, true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Allowed,
    );
    Ok(())
}

/// Restriction hierarchy: the most restrictive configs (users/ scoped)
/// fail first while less restrictive configs still accept.
#[mononoke::fbinit_test]
async fn test_most_restrictive_hook_fails_first(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 10);
    let per_user = make_config(&[], true, 6);
    let users_global = make_config(&["users/"], false, 5);
    let users_per_user = make_config(&["users/"], true, 4);

    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/draft.txt", "draft")
        .set_message(format!("eligible commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &per_user, Some("alice"), true).await?,
        RateLimitOutcome::Allowed,
    );
    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_global, None, true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    Ok(())
}

/// Large draft stack exceeding all limits: all configs reject.
#[mononoke::fbinit_test]
async fn test_large_stack_all_hooks_reject(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 10);
    let per_user = make_config(&[], true, 6);
    let users_global = make_config(&["users/"], false, 5);
    let users_per_user = make_config(&["users/"], true, 4);

    let mut parent = tip;
    let mut last_cs_id = None;
    for i in 0..5 {
        let cs = CreateCommitContext::new(ctx, repo, vec![parent])
            .add_file(format!("users/alice/stack_{}.txt", i).as_str(), "x")
            .set_message(format!("stack commit {} {}", i, ELIGIBLE_TAG))
            .set_author(ALICE)
            .commit()
            .await?;
        parent = cs;
        last_cs_id = Some(cs);
    }
    let last = last_cs_id.expect("at least one commit created");
    let bcs = last.load(ctx, repo.repo_blobstore()).await?;

    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &per_user, Some("alice"), true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_global, None, true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    assert!(matches!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Exceeded { .. },
    ));
    Ok(())
}

/// Eligible commit outside users/: directory-scoped configs skip it.
#[mononoke::fbinit_test]
async fn test_eligible_commit_outside_scoped_directory(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 10);
    let per_user = make_config(&[], true, 6);
    let users_global = make_config(&["users/"], false, 5);
    let users_per_user = make_config(&["users/"], true, 4);

    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/server/new.txt", "new")
        .set_message(format!("eligible commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // The commit touches fbcode/, not users/, so the directory-scoped rules
    // skip it; the repo-wide rules still evaluate it and allow it.
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_global, None, true).await?,
        RateLimitOutcome::Skipped,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Skipped,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &per_user, Some("alice"), true).await?,
        RateLimitOutcome::Allowed,
    );
    Ok(())
}

/// Per-user configs only count the commit's author. bob passes where alice fails.
#[mononoke::fbinit_test]
async fn test_different_user_not_blocked_by_per_user_limit(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let strict_per_user = make_config(&[], true, 3);

    let alice_draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/alice_new.txt", "x")
        .set_message(format!("alice commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let alice_bcs = alice_draft.load(ctx, repo.repo_blobstore()).await?;
    assert!(matches!(
        check_commit_rate_limit(
            ctx,
            repo,
            &bm,
            &alice_bcs,
            &strict_per_user,
            Some("alice"),
            true
        )
        .await?,
        RateLimitOutcome::Exceeded { .. },
    ));

    let bob_draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/bob_new.txt", "x")
        .set_message(format!("bob commit {}", ELIGIBLE_TAG))
        .set_author(BOB)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bob_bcs = bob_draft.load(ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        check_commit_rate_limit(
            ctx,
            repo,
            &bm,
            &bob_bcs,
            &strict_per_user,
            Some("bob"),
            true
        )
        .await?,
        RateLimitOutcome::Allowed,
    );
    Ok(())
}

/// New bookmark with no history: always accept.
#[mononoke::fbinit_test]
async fn test_new_bookmark_no_ancestors(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
    borrowed!(ctx, repo);

    let global = make_config(&[], false, 10);
    let users_per_user = make_config(&["users/"], true, 4);
    let bm = BookmarkKey::new("new_branch")?;

    let root = CreateCommitContext::new_root(ctx, repo)
        .add_file("users/alice/first.txt", "first")
        .set_message(format!("first commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = root.load(ctx, repo.repo_blobstore()).await?;

    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &global, None, true).await?,
        RateLimitOutcome::Allowed,
    );
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_per_user, Some("alice"), true).await?,
        RateLimitOutcome::Allowed,
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_check_all_commit_rate_limits_with_no_rules(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
    borrowed!(ctx, repo);

    let bm = BookmarkKey::new("main")?;
    let cs_id = CreateCommitContext::new_root(ctx, repo)
        .add_file("users/alice/first.txt", "first")
        .set_message(format!("first commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let result = check_all_commit_rate_limits(ctx, repo, &bonsai, cs_id, &bm).await?;

    assert!(result.passed, "Empty commit rate limit config should pass");
    assert!(
        result.rule_results.is_empty(),
        "Empty commit rate limit config should return no rule results",
    );
    Ok(())
}

/// When creating a new bookmark (one that doesn't exist yet), draft ancestor
/// counting should treat the draft count as 0 — there is no existing bookmark
/// position to diff against. With max_commits=1, only the commit itself is
/// counted (total = 0 public + 0 draft + 1 self = 1), so it must be Allowed.
/// If ancestors were incorrectly walked (because the bookmark lookup returns
/// None and an empty exclusion set is passed to ancestors_difference_stream),
/// the 7 eligible ancestors in the test DAG would be counted as draft, giving
/// total = 8 > 1, which would produce Exceeded.
#[mononoke::fbinit_test]
async fn test_new_bookmark_does_not_count_draft_ancestors(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, _existing_bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/new.txt", "new")
        .set_message(format!("new commit {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    let new_bm = BookmarkKey::new("brand_new_bookmark")?;
    let config = make_config(&[], false, 1);

    let outcome = check_commit_rate_limit(ctx, repo, &new_bm, &bcs, &config, None, true).await?;
    assert_eq!(outcome, RateLimitOutcome::Allowed);
    Ok(())
}

/// When the display name differs from the email prefix, per-user rate
/// limiting groups by the email prefix, not the display name.
/// e.g. "bob <robert@meta.com>" → groups by "robert", not "bob".
#[mononoke::fbinit_test]
async fn test_different_name_vs_email_uses_email(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    // Per-user hook with limit=6. Alice has 5 eligible ancestors in the DAG.
    let per_user = make_config(&[], true, 6);

    // Author display name is "bob" but email is robert@meta.com.
    // parse_author_username extracts "robert" from the email, not "bob".
    // Since "robert" has 0 prior commits, per-user count is 0 < 6 → Allowed.
    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/server/new.txt", "data")
        .set_message(format!("commit by bob/robert {}", ELIGIBLE_TAG))
        .set_author("bob <robert@meta.com>")
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // user_filter uses email prefix "robert", not display name "bob"
    assert_eq!(
        check_commit_rate_limit(
            ctx,
            repo,
            &bm,
            &bcs,
            &per_user,
            parse_author_username(bcs.author(), true).ok().flatten(),
            true,
        )
        .await?,
        RateLimitOutcome::Allowed,
    );
    Ok(())
}

/// Commits with non-standard author format (e.g. "twsvcscm@hostname" from
/// Sandcastle) cause per-user rate limiting to fall back to global counting.
/// parse_author_username fails on these, causing user_filter to be None,
/// which makes the per-user hook count ALL commits globally.
#[mononoke::fbinit_test]
async fn test_non_standard_author_tracked_per_user(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    // Global per-user hook with limit=6.
    // The test DAG has 7 total eligible ancestors (A-G), with 5 by alice
    // and 2 by bob. A non-standard author has 0 prior commits.
    let per_user = make_config(&[], true, 6);

    // A commit with non-standard author (bare user@host, no "Name <...>"
    // wrapper) should only count that author's commits, not all commits.
    let sandcastle_author = "twsvcscm@ed77-8a84.twshared2276.01.snb3.tw.fbinfra.net";
    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/server/sandcastle.txt", "data")
        .set_message(format!("sandcastle commit {}", ELIGIBLE_TAG))
        .set_author(sandcastle_author)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // With allow_bare_unixname=false, parse_author_username returns Ok(None)
    // for "twsvcscm@host" (no "Name <...>" wrapper). user_filter becomes
    // None, so the per-user hook counts ALL 7 eligible ancestors globally
    // and rejects (7 >= 6). This is the JK-off fallback behavior.
    assert!(
        matches!(
            check_commit_rate_limit(
                ctx,
                repo,
                &bm,
                &bcs,
                &per_user,
                parse_author_username(bcs.author(), false).ok().flatten(),
                false,
            )
            .await?,
            RateLimitOutcome::Exceeded { .. },
        ),
        "JK-off: non-standard author falls back to global counting"
    );
    Ok(())
}

/// With allow_bare_unixname=true (JK on), a bare unixname like "twsvcscm"
/// is parsed successfully, giving per-user counting scoped to that author.
/// Since the test DAG has 0 commits by "twsvcscm", the per-user count is
/// 0 + 1 (self) = 1 < 6 → Allowed.
#[mononoke::fbinit_test]
async fn test_non_standard_author_jk_on_tracked(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let per_user = make_config(&[], true, 6);

    // Bare unixname (no "Name <...>" wrapper, no @host).
    let bare_author = "twsvcscm";
    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("fbcode/server/sandcastle.txt", "data")
        .set_message(format!("sandcastle commit {}", ELIGIBLE_TAG))
        .set_author(bare_author)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // With allow_bare_unixname=true, parse_author_username("twsvcscm", true)
    // returns Ok(Some("twsvcscm")). Per-user counting finds 0 prior commits
    // by "twsvcscm", total = 0 + 1 = 1 < 6 → Allowed.
    assert_eq!(
        check_commit_rate_limit(
            ctx,
            repo,
            &bm,
            &bcs,
            &per_user,
            parse_author_username(bcs.author(), true).ok().flatten(),
            true,
        )
        .await?,
        RateLimitOutcome::Allowed,
        "JK-on: bare unixname is tracked per-user"
    );
    Ok(())
}

/// Draft stack bypass prevention: draft ancestors are counted so a user
/// can't bypass limits by batching commits into one push. Also verifies
/// that a non-eligible commit on top of a rejected stack still passes.
#[mononoke::fbinit_test]
async fn test_draft_stack_bypass_prevention(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let users_per_user = make_config(&["users/"], true, 4);

    let draft_1 = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/stack_1.txt", "1")
        .set_message(format!("stack 1 {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let draft_2 = CreateCommitContext::new(ctx, repo, vec![draft_1])
        .add_file("users/alice/stack_2.txt", "2")
        .set_message(format!("stack 2 {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let draft_3 = CreateCommitContext::new(ctx, repo, vec![draft_2])
        .add_file("users/alice/stack_3.txt", "3")
        .set_message(format!("stack 3 {}", ELIGIBLE_TAG))
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;

    let bcs_1 = draft_1.load(ctx, repo.repo_blobstore()).await?;
    assert!(
        matches!(
            check_commit_rate_limit(ctx, repo, &bm, &bcs_1, &users_per_user, Some("alice"), true,)
                .await?,
            RateLimitOutcome::Exceeded { .. },
        ),
        "draft_1: alice already at limit from public ancestors"
    );

    let bcs_3 = draft_3.load(ctx, repo.repo_blobstore()).await?;
    assert!(
        matches!(
            check_commit_rate_limit(ctx, repo, &bm, &bcs_3, &users_per_user, Some("alice"), true,)
                .await?,
            RateLimitOutcome::Exceeded { .. },
        ),
        "draft_3: 4 public + 2 draft ancestors"
    );

    // Non-eligible commit on top of rejected stack -> ACCEPT
    let non_eligible = CreateCommitContext::new(ctx, repo, vec![draft_3])
        .add_file("users/alice/safe.txt", "safe")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs_safe = non_eligible.load(ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        check_commit_rate_limit(
            ctx,
            repo,
            &bm,
            &bcs_safe,
            &users_per_user,
            Some("alice"),
            true
        )
        .await?,
        RateLimitOutcome::Skipped,
        "non-eligible commit must always pass",
    );
    Ok(())
}

// =========================================================================
// Cache isolation tests
// =========================================================================

/// Per-rule cache isolation: two rules with different directory scopes
/// must NOT share cached eligibility results.
///
/// Setup (from `setup_test_repo`):
///   All eligible ancestors:    A,B,C,D,E,F,G = 7
///   users/ eligible ancestors: A,B,D,E,G     = 5
///
/// Rules:
///   global_cached:  all dirs, max_commits = 6  → 7 >= 6 → EXCEED
///   users_cached:   users/,  max_commits = 6  → 5 < 6  → ALLOW
///
/// If the two rules shared a cache, the global rule would populate the
/// cache with eligibility results for fbcode/ commits (C, F), and the
/// users/ rule would reuse them, incorrectly counting 7 instead of 5
/// and returning EXCEED instead of ALLOW.
#[mononoke::fbinit_test]
async fn test_per_rule_cache_isolation(fb: FacebookInit) -> Result<()> {
    let (ctx, repo, bm, tip) = setup_test_repo(fb).await?;
    borrowed!(ctx, repo);

    let cache_config = Some(CommitRateLimitCacheConfig {
        max_entries: 50000,
        ttl_secs: 300,
    });

    // Global rule (all directories): limit 6. There are 7 eligible
    // ancestors, so this must EXCEED.
    let global_cache = cache_config.as_ref().and_then(|cc| cc.build_cache());
    let global_cached = CommitRateLimitRule::new(
        "global_cached".to_string(),
        "test_repo".to_string(),
        vec![EligibilityCheck::HgExtra {
            key: ELIGIBLE_EXTRA.to_string(),
        }],
        vec![RateLimit::new(3600, 6).expect("valid rate limit")],
        vec![],
        false,
        global_cache,
    );

    // users/-scoped rule: limit 6. There are only 5 eligible ancestors
    // touching users/, so this must ALLOW.
    let users_cache = cache_config.as_ref().and_then(|cc| cc.build_cache());
    let users_cached = CommitRateLimitRule::new(
        "users_cached".to_string(),
        "test_repo".to_string(),
        vec![EligibilityCheck::HgExtra {
            key: ELIGIBLE_EXTRA.to_string(),
        }],
        vec![RateLimit::new(3600, 6).expect("valid rate limit")],
        vec!["users/".to_string()],
        false,
        users_cache,
    );

    // An eligible draft commit touching users/
    let draft = CreateCommitContext::new(ctx, repo, vec![tip])
        .add_file("users/alice/draft.txt", "draft")
        .add_extra(ELIGIBLE_EXTRA, b"1")
        .set_author(ALICE)
        .set_author_date(recent_date())
        .commit()
        .await?;
    let bcs = draft.load(ctx, repo.repo_blobstore()).await?;

    // Check global FIRST — this populates its cache with all 7 eligible
    // ancestors, including C and F which touch fbcode/ (not users/).
    assert!(
        matches!(
            check_commit_rate_limit(ctx, repo, &bm, &bcs, &global_cached, None, true).await?,
            RateLimitOutcome::Exceeded { .. },
        ),
        "global rule must EXCEED (7 eligible >= 6 limit)"
    );

    // Check users/-scoped rule SECOND — if it shared the global rule's
    // cache, it would incorrectly count C and F as eligible (they were
    // cached as eligible by the global rule) and return EXCEED.
    // With a separate cache, it correctly sees only 5 eligible and ALLOWs.
    assert_eq!(
        check_commit_rate_limit(ctx, repo, &bm, &bcs, &users_cached, None, true).await?,
        RateLimitOutcome::Allowed,
        "users/ rule must ALLOW (5 eligible < 6 limit) — \
         if this fails, the caches are shared across rules"
    );
    Ok(())
}

// =========================================================================
// Cache tests
// =========================================================================

/// Cache miss returns None from lookup.
#[mononoke::test]
fn test_cache_miss_returns_none() {
    let cache = ChangesetEligibilityCache::new(1000, Duration::from_secs(300));
    let cs_id = mononoke_types::ChangesetId::from_bytes([1u8; 32]).expect("valid changeset id");

    assert!(cache.lookup(&cs_id).is_none(), "uncached key must miss");
}

/// Positive caching through get_or_insert_with: on_miss runs once, second
/// call returns cached value and only runs on_hit.
#[mononoke::test]
fn test_cache_positive_via_get_or_insert_with() {
    let cache = ChangesetEligibilityCache::new(1000, Duration::from_secs(300));
    let cs_id = mononoke_types::ChangesetId::from_bytes([1u8; 32]).expect("valid changeset id");
    let miss_count = AtomicU64::new(0);
    let hit_count = AtomicU64::new(0);

    let result = cache.get_or_insert_with(
        cs_id,
        || {
            hit_count.fetch_add(1, Ordering::SeqCst);
        },
        || {
            miss_count.fetch_add(1, Ordering::SeqCst);
            Some(EligibleChangesetInfo {
                parsed_username: Some("alice".to_string()),
            })
        },
    );
    assert!(result.is_some());
    assert_eq!(miss_count.load(Ordering::SeqCst), 1);
    assert_eq!(hit_count.load(Ordering::SeqCst), 0);

    // Second call: cached, on_hit runs, on_miss does NOT run
    let result = cache.get_or_insert_with(
        cs_id,
        || {
            hit_count.fetch_add(1, Ordering::SeqCst);
        },
        || {
            miss_count.fetch_add(1, Ordering::SeqCst);
            Some(EligibleChangesetInfo {
                parsed_username: Some("alice".to_string()),
            })
        },
    );
    assert!(result.is_some());
    assert_eq!(
        miss_count.load(Ordering::SeqCst),
        1,
        "on_miss must not run on cache hit"
    );
    assert_eq!(
        hit_count.load(Ordering::SeqCst),
        1,
        "on_hit must run on cache hit"
    );
}

/// Negative caching through explicit insert + lookup: inserting None (ineligible)
/// and verifying it's returned as a cache hit.
#[mononoke::test]
fn test_cache_negative_via_insert_and_lookup() {
    let cache = ChangesetEligibilityCache::new(1000, Duration::from_secs(300));
    let cs_id = mononoke_types::ChangesetId::from_bytes([2u8; 32]).expect("valid changeset id");

    cache.insert(cs_id, None);

    let cached = cache.lookup(&cs_id);
    assert!(cached.is_some(), "key must be present after insert",);
    assert!(
        cached.expect("just checked").is_none(),
        "cached value must be None (ineligible)",
    );
}

/// Different keys are independent: inserting for one key does not affect another.
#[mononoke::test]
fn test_cache_different_keys_independent() {
    let cache = ChangesetEligibilityCache::new(1000, Duration::from_secs(300));

    let cs_id_1 = mononoke_types::ChangesetId::from_bytes([1u8; 32]).expect("valid changeset id");
    let cs_id_2 = mononoke_types::ChangesetId::from_bytes([2u8; 32]).expect("valid changeset id");

    cache.insert(
        cs_id_1,
        Some(EligibleChangesetInfo {
            parsed_username: Some("alice".to_string()),
        }),
    );
    cache.insert(
        cs_id_2,
        Some(EligibleChangesetInfo {
            parsed_username: Some("bob".to_string()),
        }),
    );

    let result_1 = cache.lookup(&cs_id_1).expect("key 1 must be cached");
    let result_2 = cache.lookup(&cs_id_2).expect("key 2 must be cached");

    assert_eq!(
        result_1.as_ref().and_then(|i| i.parsed_username.as_deref()),
        Some("alice")
    );
    assert_eq!(
        result_2.as_ref().and_then(|i| i.parsed_username.as_deref()),
        Some("bob")
    );
}

#[mononoke::test]
fn test_cache_sync_api() {
    let cache = ChangesetEligibilityCache::new(1000, Duration::from_secs(300));
    let cs_id = mononoke_types::ChangesetId::from_bytes([3u8; 32]).expect("valid changeset id");
    let miss_count = AtomicU64::new(0);
    let hit_count = AtomicU64::new(0);

    // First call: on_miss runs
    let result = cache.get_or_insert_with(
        cs_id,
        || {
            hit_count.fetch_add(1, Ordering::SeqCst);
        },
        || {
            miss_count.fetch_add(1, Ordering::SeqCst);
            Some(EligibleChangesetInfo {
                parsed_username: Some("alice".to_string()),
            })
        },
    );
    assert!(result.is_some());
    assert_eq!(miss_count.load(Ordering::SeqCst), 1);
    assert_eq!(hit_count.load(Ordering::SeqCst), 0);

    // Second call: cached, on_hit runs
    let result = cache.get_or_insert_with(
        cs_id,
        || {
            hit_count.fetch_add(1, Ordering::SeqCst);
        },
        || {
            miss_count.fetch_add(1, Ordering::SeqCst);
            Some(EligibleChangesetInfo {
                parsed_username: Some("alice".to_string()),
            })
        },
    );
    assert!(result.is_some());
    assert_eq!(miss_count.load(Ordering::SeqCst), 1);
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
}

// =========================================================================
// Cache config tests
// =========================================================================

#[mononoke::test]
fn test_build_cache_disabled_when_zero_entries() {
    let cc = CommitRateLimitCacheConfig {
        max_entries: 0,
        ttl_secs: 300,
    };
    assert!(
        cc.build_cache().is_none(),
        "max_entries == 0 must disable the cache"
    );
}

#[mononoke::test]
fn test_build_cache_enabled_when_positive_entries() {
    let cc = CommitRateLimitCacheConfig {
        max_entries: 1000,
        ttl_secs: 300,
    };
    assert!(
        cc.build_cache().is_some(),
        "max_entries > 0 must produce Some(cache)"
    );
}

#[mononoke::test]
fn test_build_cache_clamps_zero_ttl() {
    let cc = CommitRateLimitCacheConfig {
        max_entries: 1000,
        ttl_secs: 0,
    };
    assert!(
        cc.build_cache().is_some(),
        "zero TTL must be clamped, not disable the cache"
    );
}
