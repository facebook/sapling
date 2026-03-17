/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::ConfigExt;
use cmdutil::FormatterOpts;
use cmdutil::Result;
use cmdutil::define_flags;
use cmdutil::get_formatter;
use formatter::FormatOptions;
use formatter::Formattable;
use formatter::StyleWrite;
use fs_err as fs;
use repo::repo::Repo;
use serde::Serialize;
use uuid::Uuid;
use worktree::Group;
use worktree::WorktreeEntry;
use worktree::check_dest_not_in_repo;
use worktree::dissolve_group;
use worktree::with_registry_lock;

define_flags! {
    pub struct WorktreeOpts {
        /// a short label for the worktree (for 'add' and 'label')
        #[argtype("TEXT")]
        label: String,

        /// remove the label instead of setting it (for 'label')
        remove: bool,

        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<WorktreeOpts>, repo: &Repo) -> Result<u8> {
    if !repo.config().get_or("worktree", "enabled", || false)? {
        abort!("worktree command requires --config worktree.enabled=true");
    }

    let subcmd = ctx.opts.args.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "list" | "ls" => run_list(&ctx, repo),
        "add" => run_add(&ctx, repo),
        "remove" | "rm" => run_remove(&ctx, repo),
        "label" => run_label(&ctx, repo),
        "" => abort!("you need to specify a subcommand (run with --help to see a list)"),
        other => abort!("unknown worktree subcommand '{}'", other),
    }
}

pub fn aliases() -> &'static str {
    "worktree"
}

pub fn doc() -> &'static str {
    r#"manage multiple linked worktrees sharing the same repository

    worktree groups allow multiple EdenFS-backed working copies to share
    the same backing store. One worktree is designated as the main worktree,
    and additional linked worktrees can be created, listed, labeled, and
    removed.

    Subcommands::

      list [-Tjson]                           List all worktrees in the group
      add PATH [--label TEXT]                 Create a new linked worktree
      remove PATH [-y]                        Remove a linked worktree
      label [PATH] TEXT [--remove]            Set or remove a worktree label

    Currently only EdenFS-backed repositories are supported."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("SUBCOMMAND [OPTIONS] [ARGS]")
}

#[derive(Serialize)]
struct ListOutputEntry {
    path: PathBuf,
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    current: bool,
}

impl Formattable for ListOutputEntry {
    fn format_plain(
        &self,
        _options: &FormatOptions,
        writer: &mut dyn StyleWrite,
    ) -> Result<(), anyhow::Error> {
        let marker = if self.current { "*" } else { " " };
        write!(
            writer,
            "{} {:<6}  {}",
            marker,
            self.role,
            self.path.display()
        )?;
        if let Some(label) = &self.label {
            write!(writer, "   {}", label)?;
        }
        writeln!(writer)?;
        Ok(())
    }
}

// --- Subcommands ---

fn run_list(ctx: &ReqCtx<WorktreeOpts>, repo: &Repo) -> Result<u8> {
    let mut formatter = get_formatter(
        repo.config(),
        "worktree",
        &ctx.opts.formatter_opts.template,
        ctx.global_opts(),
        Box::new(ctx.io().output()),
    )?;
    let output_empty = |formatter: &mut Box<dyn formatter::ListFormatter>| -> Result<()> {
        if formatter.is_plain() {
            ctx.io().write("this worktree is not part of a group\n")?;
        } else {
            formatter.begin_list()?;
            formatter.end_list()?;
        }
        Ok(())
    };

    let shared_store_path = repo.store_path();
    let current = util::path::strip_unc_prefix(fs::canonicalize(repo.path())?);

    // Hold the lock for the entire read + cleanup cycle to get a consistent
    // view of the registry and filesystem state. Without the lock, entries
    // could be built from a stale registry snapshot that is then mutated by
    // concurrent operations or by our own cleanup code.
    let entries = with_registry_lock(shared_store_path, |registry| {
        let Some(group_id) = registry.find_group_for_path(&current) else {
            return Ok(None);
        };

        let group = registry
            .groups
            .get(&group_id)
            .expect("group must exist after find_group_for_path");

        // If the main worktree is missing, dissolve the entire group.
        if !group.main.exists() {
            dissolve_group(registry, &group_id);
            return Ok(None);
        }

        // Auto-cleanup: remove stale entries whose paths no longer exist on disk.
        let has_missing = group.worktrees.keys().any(|p| !p.exists());
        if has_missing {
            let group = registry
                .groups
                .get_mut(&group_id)
                .expect("group must exist: not dissolved when main is present");
            group.worktrees.retain(|path, _| path.exists());
            let linked_count = group.worktrees.keys().filter(|p| **p != group.main).count();
            if linked_count == 0 {
                dissolve_group(registry, &group_id);
                return Ok(None);
            }
        }

        // Build entries from the (possibly cleaned) registry.
        let group = registry
            .groups
            .get(&group_id)
            .expect("group must exist: not dissolved when linked worktrees remain");
        let entries: Vec<ListOutputEntry> = group
            .worktrees
            .iter()
            .map(|(path, entry)| {
                let role = if *path == group.main {
                    "main"
                } else {
                    "linked"
                };
                ListOutputEntry {
                    path: path.clone(),
                    role,
                    label: entry.label.clone(),
                    current: *path == current,
                }
            })
            .collect();

        Ok(Some(entries))
    })?;

    match entries {
        None => {
            output_empty(&mut formatter)?;
        }
        Some(entries) => {
            formatter.begin_list()?;
            for entry in &entries {
                formatter.format_item(entry)?;
            }
            formatter.end_list()?;
        }
    }

    Ok(0)
}

fn run_add(ctx: &ReqCtx<WorktreeOpts>, repo: &Repo) -> Result<u8> {
    let logger = ctx.logger();
    let dest_str = match ctx.opts.args.get(1) {
        Some(value) => value,
        None => abort!("usage: sl worktree add PATH"),
    };
    let dest = util::path::strip_unc_prefix(util::path::canonical_path_allow_missing(dest_str)?);

    // Fast-fail before locking (re-checked inside lock).
    if dest.exists() {
        abort!("destination path '{}' already exists", dest.display());
    }
    check_dest_not_in_repo(&dest)?;

    let shared_store_path = repo.store_path().to_path_buf();

    let source_client_dir = edenfs_client::get_client_dir(repo.path())?;

    // Get the source repo's current commit so the new worktree starts at the same revision.
    let parents = workingcopy::fast_path_wdir_parents(repo.path(), repo.ident())?;
    let target = parents.p1().copied();

    // Replicate the source repo's scm type and active filters.
    // When edensparse is in requirements, the backing store should be filteredhg
    // (even with no filter paths configured). Otherwise the backing store is hg.
    let clone_filters = repo.requirements.contains("edensparse").then(|| {
        filters::util::filter_paths_from_config(repo.config().as_ref()).unwrap_or_default()
    });

    // Pre-compute the canonical path for the source repo before acquiring the lock.
    let canonical_repo_path = fs::canonicalize(repo.path())
        .map(util::path::strip_unc_prefix)
        .unwrap_or_else(|_| repo.path().to_path_buf());

    let enable_windows_symlinks = clone::read_enable_windows_symlinks(&source_client_dir)?;

    // Hold the registry lock across the clone and registry update so that
    // concurrent `worktree add` calls are serialized. The dest.exists()
    // check is repeated here while holding the lock to guard against races
    // in parallel `worktree add` calls, allowing us to cleanly exit rather
    // than letting clone fail.
    with_registry_lock(&shared_store_path, |registry| {
        if dest.exists() {
            abort!("destination path '{}' already exists", dest.display());
        }

        let existing_group_id = registry.find_group_for_path(&canonical_repo_path);
        let group_id = existing_group_id.unwrap_or_else(|| format!("{:x}", Uuid::new_v4()));

        // Create new EdenFS working copy.
        //
        // NOTE: If eden_clone fails after partially creating the checkout, EdenFS may have already
        // registered the mount. The registry won't be updated (we return early on error),
        // leaving an orphan checkout.
        //
        // If holding the registry lock for the duration of the clone is too
        // expensive, consider reserving the path in the registry (or a per-path
        // lock) before cloning, then finalizing the entry afterward.
        if let Err(err) =
            clone::eden_clone(repo, &dest, target, clone_filters, enable_windows_symlinks)
        {
            ctx.logger().warn(format!(
                "worktree add may have left a partial checkout; try running `eden rm {}` to recover",
                dest.display()
            ));
            return Err(err);
        }

        // Copy the sparse/filter config so the new worktree has the same active filters.
        clone::copy_sparse_config(repo.dot_hg_path(), &dest.join(repo.ident().dot_dir()))?;

        // Copy user-specific EdenFS config (redirections, prefetch profiles) from
        // the source worktree to the new one. Repo-level redirections from
        // .eden-redirections are applied automatically by the clone.
        clone::copy_eden_user_config(repo.config().as_ref(), &source_client_dir, &dest)?;

        let grp = registry
            .groups
            .entry(group_id.clone())
            .or_insert_with(|| Group::new(canonical_repo_path.clone()));

        grp.worktrees.insert(
            dest.clone(),
            WorktreeEntry {
                added: chrono::Utc::now().to_rfc3339(),
                label: if ctx.opts.label.is_empty() {
                    None
                } else {
                    Some(ctx.opts.label.clone())
                },
            },
        );

        Ok(())
    })?;

    logger.info(format!("created linked worktree at {}", dest.display()));
    Ok(0)
}

fn run_remove(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree remove not yet implemented");
}

fn run_label(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree label not yet implemented");
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    struct MockWriter(Vec<u8>);

    impl std::io::Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl formatter::StyleWrite for MockWriter {
        fn write_styled(&mut self, _style: &str, text: &str) -> anyhow::Result<()> {
            self.0.write_all(text.as_bytes())?;
            Ok(())
        }
    }

    fn mock_output(writer: &MockWriter) -> String {
        String::from_utf8(writer.0.clone()).unwrap()
    }

    // --- format_plain tests ---

    #[test]
    fn test_format_plain_main() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: None,
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  main    /repos/main\n");
    }

    #[test]
    fn test_format_plain_linked() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/linked"),
            role: "linked",
            label: None,
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  linked  /repos/linked\n");
    }

    #[test]
    fn test_format_plain_with_label() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: Some("my-label".to_string()),
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  main    /repos/main   my-label\n");
    }

    #[test]
    fn test_format_plain_current() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: None,
            current: true,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "* main    /repos/main\n");
    }

    #[test]
    fn test_format_plain_current_with_label() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/linked"),
            role: "linked",
            label: Some("dev".to_string()),
            current: true,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "* linked  /repos/linked   dev\n");
    }
}
