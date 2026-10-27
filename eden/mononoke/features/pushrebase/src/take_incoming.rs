/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! "Take the pushed side" merge resolution.
//!
//! A both-sides-changed conflict resolves to the pushed commit instead of
//! rejecting the pushrebase, the way `git rebase -X theirs` does on a
//! checkout: an LFS pointer is taken as-is, and a text file takes the pushed
//! lines for the conflicting regions only. The caller asks for it per push
//! (`MERGE_TAKE_INCOMING_PUSHVAR_KEY`), and it applies to every content
//! conflict in that push; a repo can also still name path prefixes
//! (`merge_resolution_take_incoming_prefixes`, deprecated) under which it
//! applies without being asked. Both gated by the
//! `pushrebase_take_incoming_enabled` JustKnob per repo. Everything here is
//! policy; the merge mechanics stay in the crate root.

use context::CoreContext;
use metaconfig_types::PushrebaseFlags;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use mononoke_types::PrefixTrie;

use crate::BaseFile;
use crate::Repo;

/// Where take-incoming applies in this pushrebase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TakeIncomingScope<'a> {
    /// The caller asked: every content conflict in the push.
    Requested,
    /// The repo's configured prefixes (deprecated path).
    Prefixes(&'a PrefixTrie),
}

impl TakeIncomingScope<'_> {
    pub(crate) fn covers(self, path: &NonRootMPath) -> bool {
        match self {
            TakeIncomingScope::Requested => true,
            TakeIncomingScope::Prefixes(trie) => trie.contains_prefix(path),
        }
    }
}

/// How merge resolution treats a path both sides changed. Decided at
/// eligibility from the [`TakeIncomingScope`]; `Merge` for every path
/// outside it, so no request and no config means the merge is unchanged.
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
        take_incoming: Option<TakeIncomingScope<'_>>,
        path: &NonRootMPath,
        pushed_is_lfs: bool,
    ) -> Self {
        let covered = take_incoming.is_some_and(|scope| scope.covers(path));
        match (covered, pushed_is_lfs) {
            (false, _) => Resolution::Merge,
            (true, false) => Resolution::PreferIncoming,
            (true, true) => Resolution::IncomingVerbatim,
        }
    }

    pub(crate) fn takes_incoming(self) -> bool {
        self != Resolution::Merge
    }
}

/// The take-incoming scope for this pushrebase on `reponame`, or `None` for
/// today's merge. The caller's request wins over the repo's prefixes. The
/// knob is read only when one of the two is present, so repos with neither
/// (all of them by default) pay nothing here. A prefix trie that matches
/// every path (the parser rejects the root prefix, so only a bug could
/// produce one) counts as unconfigured: config never applies this to a
/// whole repo; only an explicit request does.
pub(crate) fn take_incoming_scope<'a>(
    config: &'a PushrebaseFlags,
    reponame: &str,
) -> Option<TakeIncomingScope<'a>> {
    let trie = &config.merge_resolution_take_incoming_prefixes;
    let prefixes_configured = match trie {
        PrefixTrie::Included => false,
        PrefixTrie::Children(children) => !children.is_empty(),
    };
    let scope = if config.merge_resolution_take_incoming {
        TakeIncomingScope::Requested
    } else if prefixes_configured {
        TakeIncomingScope::Prefixes(trie)
    } else {
        return None;
    };
    justknobs::eval(
        "scm/mononoke:pushrebase_take_incoming_enabled",
        None,
        Some(reponame),
    )
    .then_some(scope)
}

/// A path resolved by taking the pushed side, for one unsampled Scuba row
/// per path: the rate and the content ids involved stay visible even though
/// no rebase fails.
pub(crate) struct TakeIncoming<'a> {
    pub(crate) bcs: &'a BonsaiChangeset,
    pub(crate) path: &'a NonRootMPath,
    pub(crate) resolution: Resolution,
    pub(crate) scope: TakeIncomingScope<'a>,
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
        .add(
            "scope",
            match taken.scope {
                TakeIncomingScope::Requested => "requested",
                TakeIncomingScope::Prefixes(_) => "prefixes",
            },
        )
        .add("author", taken.bcs.author())
        .add("pushed_changeset", taken.bcs.get_changeset_id().to_string())
        .add("base_content_id", taken.base.content_id.to_string())
        .add("local_content_id", taken.local.content_id.to_string())
        .add("other_content_id", taken.other.content_id.to_string())
        .log_with_msg("pushrebase_take_incoming", None);
}
