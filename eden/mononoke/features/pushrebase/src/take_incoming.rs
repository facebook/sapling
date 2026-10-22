/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Path-scoped "take the pushed side" merge resolution.
//!
//! A repo can name path prefixes (`merge_resolution_take_incoming_prefixes`)
//! under which a both-sides-changed conflict resolves to the pushed commit
//! instead of rejecting the pushrebase, the way `git merge -X ours` does: an
//! LFS pointer is taken as-is, and a text file takes the pushed lines for
//! the conflicting regions only. Gated by the
//! `pushrebase_take_incoming_enabled` JustKnob. Everything here is policy;
//! the merge mechanics stay in the crate root.

use context::CoreContext;
use metaconfig_types::PushrebaseFlags;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use mononoke_types::PrefixTrie;

use crate::BaseFile;
use crate::Repo;

/// How merge resolution treats a path both sides changed. Decided at
/// eligibility from the repo's `merge_resolution_take_incoming_prefixes`;
/// `Merge` for every path outside them, so that config empty means the
/// merge is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// Three-way merge; a conflicting region rejects the pushrebase.
    Merge,
    /// A conflicting text region resolves to the pushed side, as
    /// `git merge -X ours` would; regions only one side changed still merge.
    PreferIncoming,
    /// The pushed change is an LFS pointer: taken as-is, no content fetched
    /// or merged, so the merge size limit does not apply.
    IncomingVerbatim,
}

impl Resolution {
    pub(crate) fn for_path(
        take_incoming: Option<&PrefixTrie>,
        path: &NonRootMPath,
        pushed_is_lfs: bool,
    ) -> Self {
        let under_prefix = take_incoming.is_some_and(|trie| trie.contains_prefix(path));
        match (under_prefix, pushed_is_lfs) {
            (false, _) => Resolution::Merge,
            (true, false) => Resolution::PreferIncoming,
            (true, true) => Resolution::IncomingVerbatim,
        }
    }

    pub(crate) fn takes_incoming(self) -> bool {
        self != Resolution::Merge
    }
}

/// The take-incoming prefixes to apply for `reponame`: the repo's configured
/// trie when `pushrebase_take_incoming_enabled` is on for it, else nothing.
/// The knob is read only when the repo configures prefixes, so repos that
/// do not (all of them by default) pay nothing here. A trie that matches
/// every path (the parser rejects the root prefix, so only a bug could
/// produce one) counts as unconfigured: this rule is never applied to a
/// whole repo.
pub(crate) fn take_incoming_prefixes<'a>(
    config: &'a PushrebaseFlags,
    reponame: &str,
) -> Option<&'a PrefixTrie> {
    let trie = &config.merge_resolution_take_incoming_prefixes;
    let configured = match trie {
        PrefixTrie::Included => false,
        PrefixTrie::Children(children) => !children.is_empty(),
    };
    (configured
        && justknobs::eval(
            "scm/mononoke:pushrebase_take_incoming_enabled",
            None,
            Some(reponame),
        ))
    .then_some(trie)
}

/// A path resolved by taking the pushed side, for one unsampled Scuba row
/// per path: the rate and the content ids involved stay visible even though
/// no rebase fails.
pub(crate) struct TakeIncoming<'a> {
    pub(crate) bcs: &'a BonsaiChangeset,
    pub(crate) path: &'a NonRootMPath,
    pub(crate) resolution: Resolution,
    pub(crate) base: &'a BaseFile,
    pub(crate) local: &'a BaseFile,
    pub(crate) other: &'a BaseFile,
}

pub(crate) fn log_take_incoming(ctx: &CoreContext, repo: &impl Repo, taken: TakeIncoming<'_>) {
    ctx.scuba()
        .clone()
        .unsampled()
        .add("repo_name", repo.repo_identity().name())
        .add("path", taken.path.to_string())
        .add("resolution", format!("{:?}", taken.resolution))
        .add("author", taken.bcs.author())
        .add("pushed_changeset", taken.bcs.get_changeset_id().to_string())
        .add("base_content_id", taken.base.content_id.to_string())
        .add("local_content_id", taken.local.content_id.to_string())
        .add("other_content_id", taken.other.content_id.to_string())
        .log_with_msg("pushrebase_take_incoming", None);
}
