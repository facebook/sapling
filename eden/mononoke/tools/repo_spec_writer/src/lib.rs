/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Reusable helpers for writing Mononoke `RepoSpec` `.cconf` files via Configo.
//!
//! The SCS `create_repos` API and the Phase 6 `repo_spec_migrator` both produce
//! `RepoSpec` files at canonical paths in configerator. The path computation,
//! Python-literal formatters, and `repo_index.cinc` updater live here so both
//! consumers share a single, byte-equivalent code path.

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use repos::TShirtSize;
use sha2::Digest;
use sha2::Sha256;

/// Returns the configerator path for a RepoSpec file (no source/ prefix, no .cconf extension).
/// E.g., "scm/mononoke/repos/git/a3/org_repo" for repo name "org/repo".
/// Must match repo_spec_config_path() in generate_repo_index.py and
/// repo_spec_relative_path() in migrate_qrd_to_repo_spec.py.
pub fn make_repo_spec_config_path(repo_name: &str) -> String {
    // IMPORTANT: this hardcodes "repos/git/" because create_repos only supports GIT today.
    // Must match repo_spec_config_path() for GIT in generate_repo_index.py. Hg repos use
    // "repos/hg/{hash}/{name}" via the same hash-sharding scheme — branch on identity_scheme
    // when adding HG support.
    let hash = Sha256::digest(repo_name.as_bytes());
    let hash_dir = format!("{:02x}", hash[0]);
    format!(
        "scm/mononoke/repos/git/{}/{}",
        hash_dir,
        repo_name.replace('/', "_")
    )
}

/// Generates the file path for a RepoSpec file.
/// Path format: source/scm/mononoke/repos/git/{hash_dir}/{repo_name_escaped}.cconf
pub fn make_repo_spec_file_path(repo_name: &str) -> String {
    // IMPORTANT: see make_repo_spec_config_path() above. Path is git-only today.
    format!("source/{}.cconf", make_repo_spec_config_path(repo_name))
}

/// Returns the tier list for a new RepoSpec-based repo, as static string slices.
/// Every repo is on `gitimport`, `gitimport_content`, `scs`, and
/// `backfill_worker` — the last because mononoke_backfill_worker
/// (`fbcode/eden/mononoke/backfill_worker`) accepts ALL repos via
/// `QueueRepoFilter::Except(vec![])` and loads them on-demand when a backfill
/// request arrives. Without this entry the per-repo manifest path doesn't
/// surface the repo, the on-demand load fails, and the worker silently drops
/// backfills for it (the legacy QRD path used to populate this transitively
/// via the scs tier composer; the RepoSpec path requires explicit listing).
/// Repos whose name contains the `aosp/` substring are additionally placed in
/// the `aosp_multi_repo_land` tier so multi_repo_land_service can serve them.
/// This catches both top-level AOSP repos like `aosp/platform/...` and nested
/// ones like `oculus/aosp/vendor/oculus`.
pub fn tier_list_for_repo_spec(repo_name: &str) -> Vec<&'static str> {
    if repo_name.contains("aosp/") {
        vec![
            "gitimport",
            "gitimport_content",
            "scs",
            "backfill_worker",
            "aosp_multi_repo_land",
        ]
    } else {
        vec!["gitimport", "gitimport_content", "scs", "backfill_worker"]
    }
}

/// One entry in `repo_index.cinc`. Mirrors the Python dict shape that
/// `generate_repo_index.py` writes; field naming matches the dict keys
/// emitted by [`append_to_repo_index`].
pub struct RepoIndexEntry {
    pub config_path: String,
    pub repo_id: i32,
    pub tiers: Vec<&'static str>,
    pub is_deep_sharded: bool,
    pub t_shirt_size: TShirtSize,
    pub hipster_acl: String,
    pub enable_git_bundle_uri: Option<bool>,
}

pub fn format_python_bool(val: bool) -> &'static str {
    if val { "True" } else { "False" }
}

pub fn format_python_list(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

pub fn format_tshirt_size_python(size: TShirtSize) -> Result<&'static str> {
    match size {
        TShirtSize::SMALL => Ok("TShirtSize.SMALL"),
        TShirtSize::MEDIUM => Ok("TShirtSize.MEDIUM"),
        TShirtSize::LARGE => Ok("TShirtSize.LARGE"),
        TShirtSize::EXTRA_LARGE => Ok("TShirtSize.EXTRA_LARGE"),
        TShirtSize::EXTRA_EXTRA_LARGE => Ok("TShirtSize.EXTRA_EXTRA_LARGE"),
        TShirtSize::HUGE => Ok("TShirtSize.HUGE"),
        other => Err(anyhow!("unexpected TShirtSize variant: {other:?}")),
    }
}

/// Escape a string for embedding in a Python string literal (double-quoted).
/// Matches the escaping in generate_repo_index.py's ast_value_to_python_literal().
pub fn escape_python_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Insert new entries into a `repo_index.cinc` source string, preserving the
/// closing `}\n` so the result remains valid Python syntax.
///
/// `current_content` must contain a top-level dict literal whose closing brace
/// appears as `\n}` (matches the format `generate_repo_index.py` writes).
pub fn append_to_repo_index(
    current_content: &str,
    new_entries: &[(String, RepoIndexEntry)],
) -> Result<String> {
    let insert_pos = current_content
        .rfind("\n}")
        .ok_or_else(|| anyhow!("malformed repo_index.cinc: no closing brace"))?;

    let mut result = current_content[..insert_pos].to_string();

    for (repo_name, entry) in new_entries {
        let t_shirt_size_str = format_tshirt_size_python(entry.t_shirt_size)
            .with_context(|| format!("formatting t_shirt_size for repo {repo_name}"))?;
        let mut entry_str = format!(
            r#"
    "{}": {{
        "config_path": "{}",
        "repo_id": {},
        "tiers": {},
        "is_deep_sharded": {},
        "t_shirt_size": {},
        "default_commit_identity_scheme": RawCommitIdentityScheme.GIT,
        "hipster_acl": "{}",
        "enabled": True,
        "readonly": False,"#,
            escape_python_string(repo_name),
            escape_python_string(&entry.config_path),
            entry.repo_id,
            format_python_list(&entry.tiers),
            format_python_bool(entry.is_deep_sharded),
            t_shirt_size_str,
            escape_python_string(&entry.hipster_acl),
        );
        if let Some(bundle_uri) = entry.enable_git_bundle_uri {
            entry_str.push_str(&format!(
                "\n        \"enable_git_bundle_uri\": {},",
                format_python_bool(bundle_uri)
            ));
        }
        entry_str.push_str("\n    },");
        result.push_str(&entry_str);
    }

    result.push_str("\n}\n");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn config_path_uses_sha256_hash_dir() {
        // Must match the actual on-disk file: repos/git/04/aosp_..._wasp_proc.cconf
        let path = make_repo_spec_config_path("aosp/platform/vendor/qcom/wasp_proc");
        assert_eq!(
            path, "scm/mononoke/repos/git/04/aosp_platform_vendor_qcom_wasp_proc",
            "hash_dir for aosp/platform/vendor/qcom/wasp_proc must be 04 to match production file"
        );
    }

    #[mononoke::test]
    fn config_path_for_osmeta_matches_production() {
        // Must match the actual on-disk file: repos/git/07/osmeta_external_androidx-media.cconf
        let path = make_repo_spec_config_path("osmeta/external/androidx-media");
        assert_eq!(
            path, "scm/mononoke/repos/git/07/osmeta_external_androidx-media",
            "hash_dir for osmeta/external/androidx-media must be 07 to match production file"
        );
    }

    #[mononoke::test]
    fn file_path_wraps_with_source_and_cconf() {
        let path = make_repo_spec_file_path("manus/next-agent-webapp");
        assert!(path.starts_with("source/scm/mononoke/repos/git/"));
        assert!(path.ends_with("/manus_next-agent-webapp.cconf"));
    }

    #[mononoke::test]
    fn tier_list_aosp_includes_multi_repo_land() {
        let tiers = tier_list_for_repo_spec("aosp/platform/external/lldb-utils");
        assert_eq!(
            tiers,
            vec![
                "gitimport",
                "gitimport_content",
                "scs",
                "backfill_worker",
                "aosp_multi_repo_land"
            ]
        );
    }

    #[mononoke::test]
    fn tier_list_non_aosp_excludes_multi_repo_land() {
        let tiers = tier_list_for_repo_spec("manus/next-agent-webapp");
        assert_eq!(
            tiers,
            vec!["gitimport", "gitimport_content", "scs", "backfill_worker"]
        );
    }

    #[mononoke::test]
    fn tier_list_nested_aosp_includes_multi_repo_land() {
        let tiers = tier_list_for_repo_spec("oculus/aosp/vendor/oculus");
        assert!(
            tiers.contains(&"aosp_multi_repo_land"),
            "repos with aosp/ as a substring (e.g. oculus/aosp/vendor/oculus) must be on the aosp_multi_repo_land tier"
        );
    }

    #[mononoke::test]
    fn tier_list_always_includes_backfill_worker() {
        // Both aosp and non-aosp repos must surface in backfill_worker_manifest
        // — see the doc comment on tier_list_for_repo_spec for why omission
        // here silently breaks on-demand backfill loads.
        for repo_name in [
            "manus/next-agent-webapp",
            "aosp/platform/external/lldb-utils",
            "oculus/aosp/vendor/oculus",
            "fbsource/edenfs",
        ] {
            assert!(
                tier_list_for_repo_spec(repo_name).contains(&"backfill_worker"),
                "tier list for {repo_name} must include backfill_worker"
            );
        }
    }

    #[mononoke::test]
    fn python_bool_formatting() {
        assert_eq!(format_python_bool(true), "True");
        assert_eq!(format_python_bool(false), "False");
    }

    #[mononoke::test]
    fn python_list_quotes_each_item() {
        assert_eq!(format_python_list(&["a", "b", "c"]), r#"["a", "b", "c"]"#);
        assert_eq!(format_python_list(&[]), "[]");
    }

    #[mononoke::test]
    fn python_string_escape_handles_quote_and_backslash() {
        // Backslash must be escaped first so the escaped quote's leading
        // backslash isn't itself escaped.
        assert_eq!(escape_python_string(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape_python_string(r#"a\b"#), r#"a\\b"#);
        assert_eq!(escape_python_string(r#"a\"b"#), r#"a\\\"b"#);
    }

    #[mononoke::test]
    fn append_to_repo_index_preserves_trailing_brace() {
        let current = "REPOS = {\n    \"existing\": {\"repo_id\": 1},\n}\n";
        let entry = RepoIndexEntry {
            config_path: "scm/mononoke/repos/git/aa/new_repo".to_string(),
            repo_id: 42,
            tiers: vec!["scs", "gitimport"],
            is_deep_sharded: true,
            t_shirt_size: TShirtSize::SMALL,
            hipster_acl: "repos/git/new/repo".to_string(),
            enable_git_bundle_uri: None,
        };
        let updated = append_to_repo_index(current, &[("new/repo".to_string(), entry)]).unwrap();
        assert!(updated.ends_with("\n}\n"), "must end with closing brace");
        assert!(
            updated.contains("\"new/repo\""),
            "must contain new entry key"
        );
        assert!(updated.contains("\"repo_id\": 42"));
        assert!(
            updated.contains("\"existing\""),
            "must preserve existing entry"
        );
    }

    #[mononoke::test]
    fn append_to_repo_index_emits_bundle_uri_when_set() {
        let current = "REPOS = {\n}\n";
        let entry = RepoIndexEntry {
            config_path: "scm/mononoke/repos/git/aa/r".to_string(),
            repo_id: 1,
            tiers: vec!["scs"],
            is_deep_sharded: true,
            t_shirt_size: TShirtSize::SMALL,
            hipster_acl: "a".to_string(),
            enable_git_bundle_uri: Some(false),
        };
        let updated = append_to_repo_index(current, &[("r".to_string(), entry)]).unwrap();
        assert!(updated.contains("\"enable_git_bundle_uri\": False"));
    }

    #[mononoke::test]
    fn append_to_repo_index_omits_bundle_uri_when_none() {
        let current = "REPOS = {\n}\n";
        let entry = RepoIndexEntry {
            config_path: "scm/mononoke/repos/git/aa/r".to_string(),
            repo_id: 1,
            tiers: vec!["scs"],
            is_deep_sharded: true,
            t_shirt_size: TShirtSize::SMALL,
            hipster_acl: "a".to_string(),
            enable_git_bundle_uri: None,
        };
        let updated = append_to_repo_index(current, &[("r".to_string(), entry)]).unwrap();
        assert!(!updated.contains("enable_git_bundle_uri"));
    }

    #[mononoke::test]
    fn append_to_repo_index_rejects_malformed_input() {
        // No `\n}` closing brace
        let result = append_to_repo_index("not a dict", &[]);
        assert!(result.is_err());
    }
}
