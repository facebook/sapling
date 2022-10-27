/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::bail;
use anyhow::Result;
use async_runtime::try_block_unless_interrupted;
use checkout::ActionMap;
use checkout::Checkout;
use checkout::CheckoutPlan;
use clidispatch::errors;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest_tree::Diff;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use pathmatcher::AlwaysMatcher;
use pathmatcher::Matcher;
use progress_model::ProgressBar;
use repo::repo::Repo;
use treestate::dirstate;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;
use vfs::VFS;
use workingcopy::workingcopy::WorkingCopy;

use super::MergeToolOpts;

type ArcMatcher = Arc<dyn Matcher + Sync + Send>;

define_flags! {
    pub struct GotoOpts {
        /// discard uncommitted changes (no backup)
        #[short('C')]
        clean: bool,

        /// require clean working directory
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
    // - Checking if unclean pending changes interfere with the checkout
    // - Filtering actions by sparse profile
    // - Adding/removing actions when the sparse profile changes
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
        return Err(errors::FallbackToPython("checkout.use-rust is False".to_owned()).into());
    };

    let mut dest: Vec<&String> = ctx.opts.args.iter().collect();
    if !ctx.opts.rev.is_empty() {
        dest.push(&ctx.opts.rev);
    }

    if dest.len() != 1 {
        bail!(
            "checkout requires exactly one destination commit: {:?}",
            dest
        );
    }
    let dest: String = dest[0].clone();

    if ctx.opts.clean {
        tracing::debug!(target: "checkout_info", status_detail="unsupported_args");
        return Err(errors::FallbackToPython(
            "one or more unsupported options in Rust checkout".to_owned(),
        )
        .into());
    }

    // 1. Check if status is dirty
    let matcher = Arc::new(AlwaysMatcher::new());
    let _status = wc.status(
        matcher.clone(),
        SystemTime::UNIX_EPOCH,
        repo.config(),
        ctx.io(),
    )?;
    // TODO: Abort if status is not clean

    let current_commit = wc.parents()?.into_iter().next().unwrap_or(NULL_ID);
    let target_commit = repo.resolve_commit(&wc.treestate().lock(), &dest)?;

    let tree_resolver = repo.tree_resolver()?;
    let current_mf = tree_resolver.get(&current_commit)?;
    let target_mf = tree_resolver.get(&target_commit)?;
    let sparse_change = None; // TODO: handle sparse profile change
    // TODO: Integrate sparse matcher

    // 2. Create the plan
    let plan = create_plan(
        wc.vfs(),
        repo.config(),
        &*current_mf.read(),
        &*target_mf.read(),
        &matcher,
        sparse_change,
    )?;

    // 3. Execute the plan
    try_block_unless_interrupted(plan.apply_store(&repo.file_store()?))?;

    // 4. Update the treestate parents, dirstate
    wc.set_parents(&mut [target_commit].iter())?;
    record_updates(&plan, &wc.vfs(), &mut wc.treestate().lock())?;
    dirstate::flush(&repo.config(), wc.vfs().root(), &mut wc.treestate().lock())?;

    Ok(0)
}

fn create_plan(
    vfs: &VFS,
    config: &dyn Config,
    current_mf: &TreeManifest,
    target_mf: &TreeManifest,
    matcher: &dyn Matcher,
    sparse_change: Option<(ArcMatcher, ArcMatcher)>,
) -> Result<CheckoutPlan> {
    let diff = Diff::new(current_mf, target_mf, &matcher)?;
    let mut actions = ActionMap::from_diff(diff)?;

    if let Some((old_sparse, new_sparse)) = sparse_change {
        actions =
            actions.with_sparse_profile_change(old_sparse, new_sparse, current_mf, target_mf)?;
    }
    let checkout = Checkout::from_config(vfs.clone(), &config)?;
    let plan = checkout.plan_action_map(actions);
    // if let Some(progress_path) = progress_path {
    //     plan.add_progress(progress_path.as_path()).map_pyerr(py)?;
    // }

    Ok(plan)
}

fn record_updates(plan: &CheckoutPlan, vfs: &VFS, treestate: &mut TreeState) -> Result<()> {
    let bar = ProgressBar::register_new("recording", plan.all_files().count() as u64, "files");

    for removed in plan.removed_files() {
        treestate.remove(removed)?;
        bar.increase_position(1);
    }

    for updated in plan
        .updated_content_files()
        .chain(plan.updated_meta_files())
    {
        let fstate = checkout::file_state(vfs, updated)?;
        treestate.insert(updated, &fstate)?;
        bar.increase_position(1);
    }

    Ok(())
}

pub fn aliases() -> &'static str {
    "update|up|checkout|co|upd|upda|updat|che|chec|check|checko|checkou|goto|go"
}

pub fn doc() -> &'static str {
    r#"check out a specific commit

Update your checkout to the given destination commit. More precisely, make
the destination commit the current commit and update the contents of all
files in your checkout to match their state in the destination commit.

By default, if you attempt to check out a commit while you have pending
changes, and the destination commit is not an ancestor or descendant of
the current commit, the checkout will abort. However, if the destination
commit is an ancestor or descendant of the current commit, the pending
changes will be merged into the new checkout.

Use one of the following flags to modify this behavior:

--check: abort if there are pending changes

--clean: permanently discard any pending changes (use with caution)

--merge: attempt to merge the pending changes into the new checkout, even
if the destination commit is not an ancestor or descendant of the current
commit

If merge conflicts occur during checkout, @Product@ enters an unfinished
merge state. If this happens, fix the conflicts manually and then run
@prog@ commit to exit the unfinished merge state and save your changes in a
new commit. Alternatively, run @prog@ checkout --clean to discard your pending
changes.

Specify null as the destination commit to get an empty checkout (sometimes
known as a bare repository).

Returns 0 on success, 1 if there are unresolved files."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[-C|-c|-m] [[-r] REV]")
}
