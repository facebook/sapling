/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::ConfigExt;
use cmdutil::FormatterOpts;
use cmdutil::Result;
use cmdutil::define_flags;
use repo::repo::Repo;

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

fn run_list(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree list not yet implemented");
}

fn run_add(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree add not yet implemented");
}

fn run_remove(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree remove not yet implemented");
}

fn run_label(_ctx: &ReqCtx<WorktreeOpts>, _repo: &Repo) -> Result<u8> {
    abort!("worktree label not yet implemented");
}
