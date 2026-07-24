/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod add;
mod label;
mod list;
mod remove;

use std::path::PathBuf;

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::ConfigExt;
use cmdutil::FormatterOpts;
use cmdutil::Result;
use cmdutil::define_flags;
use fs_err as fs;
use repo::repo::Repo;
use workingcopy::workingcopy::WorkingCopy;

define_flags! {
    pub struct WorktreeOpts {
        /// a short label for the worktree (for 'add' and 'label')
        #[argtype("TEXT")]
        label: String,

        /// create a snapshot of the current working copy, then restore it in the new worktree (for 'add')
        snapshot: bool,

        /// revision to check out (for 'add')
        #[short('r')]
        #[argtype("REV")]
        rev: String,

        /// remove all linked worktrees (for 'remove')
        all: bool,

        /// remove the label instead of setting it (for 'label')
        remove: bool,

        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<WorktreeOpts>, repo: &Repo, wc: &WorkingCopy) -> Result<u8> {
    if !repo.config().get_or("worktree", "enabled", || false)? {
        abort!("worktree command requires --config worktree.enabled=true");
    }

    let subcmd = ctx.opts.args.first().map(|s| s.as_str()).unwrap_or("");
    if !ctx.opts.rev.is_empty() && subcmd != "add" {
        abort!("--rev can only be used with 'worktree add'");
    }
    let runner: fn(&ReqCtx<WorktreeOpts>, &Repo, &WorkingCopy) -> Result<u8> = match subcmd {
        "list" | "ls" => list::run,
        "add" => add::run,
        "remove" | "rm" => remove::run,
        "label" => label::run,
        "" => abort!("you need to specify a subcommand (run with --help to see a list)"),
        other => abort!("unknown worktree subcommand '{}'", other),
    };

    if !repo.requirements.contains("eden") {
        abort!("worktree commands require an EdenFS-backed repository");
    }

    runner(&ctx, repo, wc)
}

pub(crate) struct CurrentGroup {
    pub(crate) shared_store_path: PathBuf,
    pub(crate) group_id: String,
}

pub(crate) fn require_group(repo: &Repo) -> Result<CurrentGroup> {
    let shared_store_path = repo.store_path();
    let registry = worktree::load_registry(shared_store_path)?;
    let current = util::path::strip_unc_prefix(fs::canonicalize(repo.path())?);
    match registry.find_group_for_path(&current) {
        Some(group_id) => Ok(CurrentGroup {
            shared_store_path: shared_store_path.to_path_buf(),
            group_id,
        }),
        None => abort!("this worktree is not part of a group"),
    }
}

pub fn aliases() -> &'static str {
    "worktree|wt"
}

pub fn doc() -> &'static str {
    r#"manage multiple linked worktrees sharing the same repository

    worktree groups allow multiple EdenFS-backed working copies to share
    the same backing store. One worktree is designated as the main worktree,
    and additional linked worktrees can be created, listed, labeled, and
    removed.

    Subcommands::

      list [-Tjson]                                     List all worktrees in the group
      add [PATH] [-r REV] [--label TEXT] [--snapshot]   Create a new linked worktree
      remove PATH [PATH...] [--all] [-y]                Remove linked worktree(s)
      label [PATH] TEXT [--remove]                      Set or remove a worktree label

    If PATH is omitted from `add`, `worktree.path-generator` is used to
    choose the destination path.

    Currently only EdenFS-backed repositories are supported."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("SUBCOMMAND [OPTIONS] [ARGS]")
}
