/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use scuba_ext::{ScubaSampleBuilder, ScubaValue};
use source_control as thrift;

use crate::commit_id::CommitIdExt;

/// A trait for logging a thrift `Params` struct to scuba.
///
/// Common and meaningful parameters (e.g. `other_commit` or
/// `identity_schemes` are logged using that name.  Other parameters
/// should have their name prefixed by `param_` to make it clear that
/// the column is for a parameter.  The default implementation does
/// nothing.
pub(crate) trait AddScubaParams: Send + Sync {
    fn add_scuba_params(&self, _scuba: &mut ScubaSampleBuilder) {}
}

impl AddScubaParams for BTreeSet<thrift::CommitIdentityScheme> {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add(
            "identity_schemes",
            self.iter().map(ToString::to_string).collect::<ScubaValue>(),
        );
    }
}

impl AddScubaParams for thrift::ListReposParams {}

impl AddScubaParams for thrift::RepoCreateCommitParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add(
            "param_parents",
            self.parents
                .iter()
                .map(CommitIdExt::to_string)
                .collect::<ScubaValue>(),
        );
        if let Some(date) = self.info.date.as_ref() {
            scuba.add("param_date", date.timestamp);
        }
        scuba.add("param_author", self.info.author.as_str());
        let deletes_count = self
            .changes
            .values()
            .filter(|change| {
                if let thrift::RepoCreateCommitParamsChange::deleted(_) = change {
                    true
                } else {
                    false
                }
            })
            .count();
        scuba.add("param_changes_count", self.changes.len() - deletes_count);
        scuba.add("param_deletes_count", deletes_count);
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoCreateBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add("commit", self.target.to_string());
    }
}

impl AddScubaParams for thrift::RepoMoveBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add("commit", self.target.to_string());
        if let Some(old_target) = &self.old_target {
            scuba.add("param_old_target", old_target.to_string());
        }
        scuba.add(
            "param_allow_non_fast_forward_move",
            self.allow_non_fast_forward_move as i32,
        );
    }
}

impl AddScubaParams for thrift::RepoDeleteBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        if let Some(old_target) = &self.old_target {
            scuba.add("param_old_target", old_target.to_string());
        }
    }
}

impl AddScubaParams for thrift::RepoLandStackParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add("commit", self.head.to_string());
        scuba.add("param_base", self.base.to_string());
        self.identity_schemes.add_scuba_params(scuba);
        if let Some(old_identity_schemes) = &self.old_identity_schemes {
            scuba.add(
                "old_identity_schemes",
                old_identity_schemes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<ScubaValue>(),
            );
        }
    }
}

impl AddScubaParams for thrift::RepoListBookmarksParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_include_scratch", self.include_scratch as i32);
        scuba.add("param_bookmark_prefix", self.bookmark_prefix.as_str());
        scuba.add("param_limit", self.limit);
        if let Some(after) = &self.after {
            scuba.add("param_after", after.as_str());
        }
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoResolveBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark_name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoResolveCommitPrefixParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_prefix", self.prefix.as_str());
        scuba.add("param_prefix_scheme", self.prefix_scheme.to_string());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoStackInfoParams {}

impl AddScubaParams for thrift::CommitCompareParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        if let Some(other_commit_id) = self.other_commit_id.as_ref() {
            scuba.add("other_commit", other_commit_id.to_string());
        }
        if let Some(paths) = &self.paths {
            scuba.add("param_paths", paths.iter().collect::<ScubaValue>());
        }
        scuba.add("param_skip_copies_renames", self.skip_copies_renames as i32);
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitFileDiffsParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("other_commit", self.other_commit_id.to_string());
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_context", self.context);
    }
}

impl AddScubaParams for thrift::CommitFindFilesParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_limit", self.limit);
        if let Some(basenames) = &self.basenames {
            scuba.add("param_basenames", basenames.iter().collect::<ScubaValue>());
        }
        if let Some(prefixes) = &self.prefixes {
            scuba.add("param_prefixes", prefixes.iter().collect::<ScubaValue>());
        }
    }
}

impl AddScubaParams for thrift::CommitInfoParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitIsAncestorOfParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("other_commit", self.other_commit_id.to_string());
    }
}

impl AddScubaParams for thrift::CommitCommonBaseWithParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("other_commit", self.other_commit_id.to_string());
    }
}

impl AddScubaParams for thrift::CommitLookupParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitHistoryParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_skip", self.skip);
        scuba.add("param_limit", self.limit);
        if let Some(before) = self.before_timestamp {
            scuba.add("param_before_timestamp", before);
        }
        if let Some(after) = self.after_timestamp {
            scuba.add("param_after_timestamp", after);
        }
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitListDescendantBookmarksParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_include_scratch", self.include_scratch as i32);
        scuba.add("param_bookmark_prefix", self.bookmark_prefix.as_str());
        scuba.add("param_limit", self.limit);
        if let Some(after) = &self.after {
            scuba.add("param_after", after.as_str());
        }
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitLookupXRepoParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("other_repo", self.other_repo.name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitPathBlameParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_format", self.format.to_string());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitPathHistoryParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_skip", self.skip);
        scuba.add("param_limit", self.limit);
        if let Some(before) = self.before_timestamp {
            scuba.add("param_before_timestamp", before);
        }
        if let Some(after) = self.after_timestamp {
            scuba.add("param_after_timestamp", after);
        }
        scuba.add(
            "follow_history_across_deletions",
            self.follow_history_across_deletions,
        );
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitPathInfoParams {}

impl AddScubaParams for thrift::CommitMultiplePathInfoParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_paths", self.paths.iter().collect::<ScubaValue>());
    }
}

impl AddScubaParams for thrift::FileContentChunkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_size", self.size);
    }
}

impl AddScubaParams for thrift::FileExistsParams {}

impl AddScubaParams for thrift::FileInfoParams {}

impl AddScubaParams for thrift::FileDiffParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add(
            "other_file",
            faster_hex::hex_string(&self.other_file_id).expect("hex_string should never fail"),
        );
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_context", self.context);
    }
}

impl AddScubaParams for thrift::TreeListParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_limit", self.limit);
    }
}

impl AddScubaParams for thrift::RepoListHgManifestParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add(
            "hg_manifest_id",
            faster_hex::hex_string(&self.hg_manifest_id).expect("hex_string should never fail"),
        );
    }
}
