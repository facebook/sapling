/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clidispatch::abort;
use clidispatch::fallback;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configmodel::ConfigExt;
use fs_err as fs;
use repo::repo::Repo;
use repostate::command_state::Operation;
use workingcopy::workingcopy::LockedWorkingCopy;
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

    if ctx.opts.check || ctx.opts.merge || !ctx.opts.date.is_empty() {
        tracing::debug!(target: "checkout_info", checkout_detail="unsupported_args");
        fallback!("one or more unsupported options in Rust checkout");
    }

    if ctx.opts.clean && ctx.opts.r#continue {
        abort!("can't specify both --clean and --continue")
    }

    let mut dest: Vec<String> = ctx.opts.args.clone();
    if !ctx.opts.rev.is_empty() {
        dest.push(ctx.opts.rev.clone());
    }

    if !dest.is_empty() && ctx.opts.r#continue {
        abort!("can't specify a destination commit and --continue");
    }

    // Protect the various ".hg" state file checks.
    let wc = wc.lock()?;

    // Clean up the "updatemergestate" file if we are done merging.
    // We do this before try_operation since that will error on "updatemergestate".
    // This should happen even without "--continue".
    let cleaned_mergestate = maybe_clear_update_merge_state(&wc, ctx.opts.clean)?;

    let updatestate_path = wc.dot_hg_path().join("updatestate");

    if ctx.opts.r#continue {
        if cleaned_mergestate {
            // User ran "sl goto --continue" after resolving all "--merge" conflicts.
            return Ok(0);
        }

        let interrupted_dest = match fs::read_to_string(&updatestate_path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                bail!("not in an interrupted goto state")
            }
            Err(err) => return Err(err.into()),
        };
        dest.push(interrupted_dest);
    }

    // We either consumed "updatestate" above, or are goto'ing someplace else,
    // so clear it out.
    util::file::unlink_if_exists(updatestate_path)?;

    let op = if ctx.opts.clean {
        Operation::GotoClean
    } else {
        Operation::Other
    };

    // This aborts if there are unresolved conflicts, or have some other
    // operation (e.g. "graft") in progress.
    repostate::command_state::try_operation(wc.locked_dot_hg_path(), op)?;

    if dest.len() > 1 {
        abort!(
            "goto requires exactly one destination commit but got: {:?}",
            dest
        );
    }

    if dest.is_empty() {
        abort!(r#"you must specify a destination to update to, for example "@prog@ goto main"."#);
    }

    let dest = dest.remove(0);

    let target = match repo.resolve_commit(Some(&wc.treestate().lock()), &dest) {
        Ok(target) => target,
        Err(err) => {
            tracing::debug!(target: "checkout_info", checkout_detail="resolve_commit");
            tracing::debug!(?err, dest, "unable to resolve checkout destination");
            fallback!("unable to resolve checkout destination");
        }
    };

    tracing::debug!(target: "checkout_info", checkout_mode="rust");

    let checkout_mode = if ctx.opts.clean {
        checkout::CheckoutMode::Force
    } else if ctx.opts.merge {
        checkout::CheckoutMode::Merge
    } else {
        checkout::CheckoutMode::NoConflict
    };
    let _lock = repo.lock()?;
    let update_result = checkout::checkout(
        &ctx.logger(),
        repo,
        &wc,
        target,
        if ctx.opts.inactive { None } else { Some(dest) },
        checkout_mode,
    )?;

    if !ctx.global_opts().quiet {
        if let Some((updated, removed)) = update_result {
            ctx.io().write(format!(
                "{} files updated, 0 files merged, {} files removed, 0 files unresolved\n",
                updated, removed
            ))?;
        } else {
            ctx.io().write("update complete\n")?;
        }
    }

    Ok(0)
}

// Clear use out of the "updatemergestate" state if there are no unresolved
// files or user specified "--clean". Returns whether state was cleared.
fn maybe_clear_update_merge_state(wc: &LockedWorkingCopy, clean: bool) -> Result<bool> {
    let ums_path = wc.dot_hg_path().join("updatemergestate");

    if !ums_path.try_exists().context("updatemergestate")? {
        return Ok(false);
    }

    if clean || !wc.read_merge_state()?.unwrap_or_default().is_unresolved() {
        fs_err::remove_file(&ums_path)?;
        Ok(true)
    } else {
        Ok(false)
    }
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
