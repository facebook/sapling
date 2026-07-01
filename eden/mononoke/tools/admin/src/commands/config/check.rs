/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `mononoke_admin config check` — validate per-repo `.cconf` changes by
//! loading each touched repo through the same `MononokeApp` +
//! `RepoFactory` code path the Mononoke server uses at startup.
//!
//! ## What this catches that `conf build` doesn't
//!
//! - Semantic errors that pass Thrift validation but fail at apply time:
//!   unresolvable `storage_config` references, missing redaction
//!   keylists, hipster_acl that doesn't resolve, derived_data_config
//!   inconsistencies the builder rejects.
//! - Per-repo facet build failures the running server would hit on
//!   hot-reload of the same config.
//!
//! ## Typical usage
//!
//! From your configerator checkout, after `conf build`:
//!
//! ```bash
//! sl status -n -m | mononoke_admin \
//!     --local-configerator-path . --prod \
//!     config check --stdin
//! ```
//!
//! Or check a single repo by name:
//!
//! ```bash
//! mononoke_admin --local-configerator-path . --prod \
//!     config check --repo aosp/manifest
//! ```
//!
//! `--local-configerator-path .` makes MononokeApp read materialized
//! configs from the current configerator working copy (i.e. your
//! in-progress edits), so the gate fires on what you're about to submit.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArg;

/// Source-path prefixes that identify per-repo `.cconf` files we know how
/// to validate. Matches the prefix set used by
/// `fbcode/eden/mononoke/facebook/canary_hook/src/handlers.rs::REPO_SPEC_PATH_PREFIXES`.
const PER_REPO_SOURCE_PREFIXES: &[&str] = &[
    "source/scm/mononoke/repos/git/",
    "source/scm/mononoke/repos/hg/",
];

// Facet container: reuse `mononoke_api::Repo` so the load exercises
// the same broad facet set production services (SCS, etc.) build at
// startup. Maximizes the chance of catching config-side semantic
// errors that only surface during facet construction (storage_config
// resolution, redaction keylist load, ACL resolution, sharding, etc.).

#[derive(Parser)]
pub struct CheckArgs {
    /// Path to a file containing one changed source path per line
    /// (typically `sl status -n -m`). Comments (`#`) and blank lines are
    /// ignored. Paths outside the per-repo `.cconf` prefixes are silently
    /// skipped.
    #[clap(long, conflicts_with_all = ["repo", "stdin"])]
    from_changed_files: Option<PathBuf>,

    /// Read changed paths from stdin (one per line). Same filtering as
    /// `--from-changed-files`.
    #[clap(long, conflicts_with_all = ["repo", "from_changed_files"])]
    stdin: bool,

    /// Check a single repo by name. Skips path-based discovery and loads
    /// the named repo through MononokeApp.
    #[clap(long, conflicts_with_all = ["from_changed_files", "stdin"])]
    repo: Option<String>,
}

pub async fn run(app: MononokeApp, args: CheckArgs) -> Result<()> {
    let repo_names = derive_repo_names(&args)?;

    if repo_names.is_empty() {
        eprintln!(
            "No per-repo .cconf changes found (looked under {}). Nothing to check.",
            PER_REPO_SOURCE_PREFIXES.join(", ")
        );
        return Ok(());
    }

    let mut fail_count = 0usize;
    for name in &repo_names {
        match check_repo(&app, name).await {
            Ok(()) => println!("PASS  {name}"),
            Err(err) => {
                println!("FAIL  {name}: {err:#}");
                fail_count += 1;
            }
        }
    }

    eprintln!();
    eprintln!(
        "{} pass / {} fail across {} repo(s) checked",
        repo_names.len() - fail_count,
        fail_count,
        repo_names.len(),
    );

    if fail_count > 0 {
        bail!("{fail_count} repo(s) failed config-check — see PASS/FAIL list above");
    }
    Ok(())
}

async fn check_repo(app: &MononokeApp, repo_name: &str) -> Result<()> {
    let repo_arg = RepoArg::Name(repo_name.to_string());
    let _repo: Repo = app.open_repo(&repo_arg).await.with_context(|| {
        format!(
            "open_repo failed for {repo_name:?} — this is the same code path the \
             Mononoke server takes at startup. The error above is what the server \
             would hit on next hot-reload."
        )
    })?;
    Ok(())
}

fn derive_repo_names(args: &CheckArgs) -> Result<Vec<String>> {
    if let Some(name) = &args.repo {
        return Ok(vec![name.clone()]);
    }
    let paths = collect_changed_paths(args)?;
    let names: BTreeSet<String> = paths
        .iter()
        .filter(|p| is_per_repo_cconf(p))
        .filter_map(|p| derive_repo_name_from_path(p))
        .collect();
    Ok(names.into_iter().collect())
}

fn collect_changed_paths(args: &CheckArgs) -> Result<Vec<String>> {
    if args.stdin {
        let stdin = std::io::stdin();
        return Ok(read_paths(stdin.lock()));
    }
    if let Some(file) = &args.from_changed_files {
        let f = std::fs::File::open(file)
            .with_context(|| format!("opening --from-changed-files {}", file.display()))?;
        return Ok(read_paths(std::io::BufReader::new(f)));
    }
    Ok(Vec::new())
}

fn read_paths<R: BufRead>(reader: R) -> Vec<String> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            // sl status output is `<status> <path>` (e.g. "M repos/git/...");
            // strip a single-char status flag + space so callers can pipe
            // raw `sl status` output without parsing.
            if trimmed.len() > 2 && trimmed.as_bytes()[1] == b' ' {
                Some(trimmed[2..].to_string())
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

fn is_per_repo_cconf(path: &str) -> bool {
    PER_REPO_SOURCE_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && path.ends_with(".cconf")
}

/// Derive a repo name from a per-repo `.cconf` source path.
///
/// The migrator sanitizes `/` in repo names to `_` in filenames (e.g.
/// `aosp/manifest` → `aosp_manifest.cconf`), so we can't unambiguously
/// reverse the mapping from the filename alone. Fall back to the
/// sanitized basename and let `MononokeApp::open_repo` surface a clear
/// error if the name doesn't match a known repo. The caller can override
/// with `--repo <actual/name>` when ambiguity bites.
fn derive_repo_name_from_path(source_path: &str) -> Option<String> {
    Path::new(source_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn is_per_repo_cconf_recognizes_git_and_hg() {
        assert!(is_per_repo_cconf(
            "source/scm/mononoke/repos/git/28/aosp_manifest.cconf"
        ));
        assert!(is_per_repo_cconf(
            "source/scm/mononoke/repos/hg/06/paws.cconf"
        ));
        assert!(!is_per_repo_cconf(
            "source/scm/mononoke/repos/repos/repo_definitions.cconf"
        ));
        assert!(!is_per_repo_cconf("source/somewhere/else.cconf"));
        assert!(!is_per_repo_cconf(
            "materialized_configs/scm/mononoke/repos/hg/06/paws.materialized_JSON"
        ));
    }

    #[mononoke::test]
    fn read_paths_strips_sl_status_prefix_and_filters_comments() {
        let input = b"M source/scm/mononoke/repos/git/28/aosp_manifest.cconf\n\
                      A source/scm/mononoke/repos/hg/06/paws.cconf\n\
                      ? unrelated/file.txt\n\
                      \n\
                      # a comment\n\
                      source/scm/mononoke/repos/git/00/foo.cconf\n";
        let paths = read_paths(&input[..]);
        assert_eq!(paths.len(), 4);
        assert!(
            paths.contains(&"source/scm/mononoke/repos/git/28/aosp_manifest.cconf".to_string())
        );
        assert!(paths.contains(&"source/scm/mononoke/repos/hg/06/paws.cconf".to_string()));
        assert!(paths.contains(&"unrelated/file.txt".to_string()));
        assert!(paths.contains(&"source/scm/mononoke/repos/git/00/foo.cconf".to_string()));
    }

    #[mononoke::test]
    fn derive_repo_name_strips_extension() {
        assert_eq!(
            derive_repo_name_from_path("source/scm/mononoke/repos/git/28/aosp_manifest.cconf"),
            Some("aosp_manifest".to_string())
        );
        assert_eq!(
            derive_repo_name_from_path("source/scm/mononoke/repos/hg/f4/fbsource.cconf"),
            Some("fbsource".to_string())
        );
    }
}
