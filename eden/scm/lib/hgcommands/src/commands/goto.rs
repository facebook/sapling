/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use anyhow::bail;
use anyhow::Result;
use clidispatch::abort;
use clidispatch::fallback;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configmodel::ConfigExt;
use fs_err as fs;
use repo::repo::Repo;
use workingcopy::workingcopy::WorkingCopy;

use super::MergeToolOpts;

define_flags! {
    pub struct GotoOpts {
        /// discard uncommitted changes (no backup)
        #[short('C')]
        clean: bool,

        /// require clean working copy
        #[short('c')]
        check: bool,

        /// merge uncommitted changes
        #[short('m')]
        merge: bool,

        /// tipmost revision matching date (ADVANCED)
        #[short('d')]
        #[argtype("DATE")]
        date: String,

        /// revision
        #[short('r')]
        #[argtype("REV")]
        rev: String,

        /// update without activating bookmarks
        inactive: bool,

        /// resume interrupted update --merge (ADVANCED)
        r#continue: bool,

        merge_opts: MergeToolOpts,

        /// create new bookmark
        #[short('B')]
        #[argtype("VALUE")]
        bookmark: String,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<GotoOpts>, repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    // Missing features (in roughly priority order):
    // - edenfs checkout support
    // - --clean support
    // - progressfile and --continue
    // - updatestate file maintaince
    // - Activating/deactivating bookmarks
    // - Checking unknown files (do we need this?)
    //
    // Features to deprecate/not support:
    // - --merge, --inactive, --date, --check

    if !repo.config().get_or_default("checkout", "use-rust")? {
        fallback!("checkout.use-rust is False");
    }

    // Protect the various ".hg" state file checks.
    let _wlock = wc.lock();

    if wc.dot_hg_path().join("updatemergestate").exists() {
        tracing::debug!(target: "checkout_info", checkout_detail="updatemergestate");
        fallback!("updatemergestate exists");
    }

    let mut dest: Vec<String> = ctx.opts.args.clone();
    if !ctx.opts.rev.is_empty() {
        dest.push(ctx.opts.rev.clone());
    }

    if ctx.opts.r#continue {
        let interrupted_dest = match fs::read_to_string(wc.dot_hg_path().join("updatestate")) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                bail!("not in an interrupted update state")
            }
            Err(err) => return Err(err.into()),
        };
        dest.push(interrupted_dest);
    }

    if dest.len() > 1 {
        abort!(
            "checkout requires exactly one destination commit but got: {:?}",
            dest
        );
    }

    if dest.is_empty() {
        abort!(r#"You must specify a destination to update to, for example "@prog@ goto main"."#);
    }

    let dest = dest.remove(0);

    if ctx.opts.clean || ctx.opts.check || ctx.opts.merge || !ctx.opts.date.is_empty() {
        tracing::debug!(target: "checkout_info", checkout_detail="unsupported_args");
        fallback!("one or more unsupported options in Rust checkout");
    }

    let target = match repo.resolve_commit(Some(&wc.treestate().lock()), &dest) {
        Ok(target) => target,
        Err(_) => {
            tracing::debug!(target: "checkout_info", checkout_detail="resolve_commit");
            fallback!("unable to resolve checkout destination");
        }
    };

    tracing::debug!(target: "checkout_info", checkout_mode="rust");

    let _lock = repo.lock();
    let (updated, removed) = checkout::checkout(ctx.io(), repo, wc, target)?;

    if !ctx.global_opts().quiet {
        ctx.io().write(format!(
            "{} files updated, 0 files merged, {} files removed, 0 files unresolved\n",
            updated, removed
        ))?;
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "goto|go|update|up|checkout|co|upd|upda|updat|che|chec|check|checko|checkou"
}

pub fn doc() -> &'static str {
    r#"update working copy to a given commit

Update your working copy to the given destination commit. More
precisely, make the destination commit the current commit and update the
contents of all files in your working copy to match their state in the
destination commit.

By default, if you attempt to go to a commit while you have pending
changes, and the destination commit is not an ancestor or descendant of
the current commit, the checkout will abort. However, if the destination
commit is an ancestor or descendant of the current commit, the pending
changes will be merged with the destination.

Use one of the following flags to modify this behavior::

    --check: abort if there are pending changes

    --clean: permanently discard any pending changes (use with caution)

    --merge: always attempt to merge the pending changes into the destination

If merge conflicts occur during update, @Product@ enters an unfinished
merge state. If this happens, fix the conflicts manually and then run
:prog:`commit` to exit the unfinished merge state and save your changes
in a new commit. Alternatively, run :prog:`goto --clean` to discard your
pending changes.

Specify null as the destination commit to get an empty working copy
(sometimes known as a bare repository).

Returns 0 on success, 1 if there are unresolved files."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... [REV]")
}
