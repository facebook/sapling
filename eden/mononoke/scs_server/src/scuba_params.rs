/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::scuba_common::hex;
use crate::scuba_common::report_megarepo_target;
use crate::scuba_common::Reported;

/// A trait for logging a thrift `Params` struct to scuba.
///
/// Common and meaningful parameters (e.g. `other_commit` or
/// `identity_schemes` are logged using that name.  Other parameters
/// should have their name prefixed by `param_` to make it clear that
/// the column is for a parameter.  The default implementation does
/// nothing.
pub(crate) trait AddScubaParams: Send + Sync {
    fn add_scuba_params(&self, _scuba: &mut MononokeScubaSampleBuilder) {}
}

impl AddScubaParams for BTreeSet<thrift::CommitIdentityScheme> {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add(
            "identity_schemes",
            self.iter().map(ToString::to_string).collect::<ScubaValue>(),
        );
    }
}

impl AddScubaParams for thrift::ListReposParams {}

impl AddScubaParams for thrift::RepoCreateCommitParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
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
        if let Some(service_identity) = self.service_identity.as_deref() {
            scuba.add("service_identity", service_identity);
        }
    }
}

impl AddScubaParams for thrift::RepoCreateBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add("commit", self.target.to_string());
        if let Some(service_identity) = self.service_identity.as_deref() {
            scuba.add("service_identity", service_identity);
        }
    }
}

impl AddScubaParams for thrift::RepoMoveBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add("commit", self.target.to_string());
        if let Some(old_target) = &self.old_target {
            scuba.add("param_old_target", old_target.to_string());
        }
        scuba.add(
            "param_allow_non_fast_forward_move",
            self.allow_non_fast_forward_move as i32,
        );
        if let Some(service_identity) = self.service_identity.as_deref() {
            scuba.add("service_identity", service_identity);
        }
    }
}

impl AddScubaParams for thrift::RepoDeleteBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        if let Some(old_target) = &self.old_target {
            scuba.add("param_old_target", old_target.to_string());
        }
        if let Some(service_identity) = self.service_identity.as_deref() {
            scuba.add("service_identity", service_identity);
        }
    }
}

impl AddScubaParams for thrift::RepoLandStackParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
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
        if let Some(service_identity) = self.service_identity.as_deref() {
            scuba.add("service_identity", service_identity);
        }
    }
}

impl AddScubaParams for thrift::RepoListBookmarksParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
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
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark_name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoBookmarkInfoParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark_name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoResolveCommitPrefixParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_prefix", self.prefix.as_str());
        scuba.add("param_prefix_scheme", self.prefix_scheme.to_string());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoStackInfoParams {}

impl AddScubaParams for thrift::RepoPrepareCommitsParams {}

impl AddScubaParams for thrift::CommitCompareParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        if let Some(other_commit_id) = self.other_commit_id.as_ref() {
            scuba.add("other_commit", other_commit_id.to_string());
        }
        if let Some(paths) = &self.paths {
            scuba.add("param_paths", paths.iter().collect::<ScubaValue>());
        }
        if let Some(ordered_params) = &self.ordered_params {
            if let Some(after_path) = &ordered_params.after_path {
                scuba.add("param_after", after_path.as_str());
            }
            scuba.add("param_limit", ordered_params.limit);
        }
        scuba.add("param_skip_copies_renames", self.skip_copies_renames as i32);
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitFileDiffsParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add(
            "other_commit",
            self.other_commit_id.as_ref().map(|oci| oci.to_string()),
        );
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_context", self.context);
    }
}

impl AddScubaParams for thrift::CommitFindFilesParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_limit", self.limit);
        if let Some(basenames) = &self.basenames {
            scuba.add("param_basenames", basenames.iter().collect::<ScubaValue>());
        }
        if let Some(suffixes) = &self.basename_suffixes {
            scuba.add("param_suffixes", suffixes.iter().collect::<ScubaValue>());
        }
        if let Some(prefixes) = &self.prefixes {
            scuba.add("param_prefixes", prefixes.iter().collect::<ScubaValue>());
        }
        if let Some(after) = &self.after {
            scuba.add("param_after", after.as_str());
        }
    }
}

impl AddScubaParams for thrift::CommitInfoParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitIsAncestorOfParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("other_commit", self.descendant_commit_id.to_string());
    }
}

impl AddScubaParams for thrift::CommitCommonBaseWithParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("other_commit", self.other_commit_id.to_string());
    }
}

impl AddScubaParams for thrift::CommitLookupParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitLookupPushrebaseHistoryParams {}

impl AddScubaParams for thrift::CommitHistoryParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
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
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_include_scratch", self.include_scratch as i32);
        scuba.add("param_bookmark_prefix", self.bookmark_prefix.as_str());
        scuba.add("param_limit", self.limit);
        if let Some(after) = &self.after {
            scuba.add("param_after", after.as_str());
        }
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitRunHooksParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
    }
}

impl AddScubaParams for thrift::CommitLookupXRepoParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("other_repo", self.other_repo.name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitPathBlameParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_format", self.format.to_string());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitPathHistoryParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
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

impl AddScubaParams for thrift::CommitPathExistsParams {}

impl AddScubaParams for thrift::CommitPathInfoParams {}

impl AddScubaParams for thrift::RepoInfoParams {}

impl AddScubaParams for thrift::CommitMultiplePathInfoParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_paths", self.paths.iter().collect::<ScubaValue>());
    }
}

impl AddScubaParams for thrift::CommitPathLastChangedParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitMultiplePathLastChangedParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_paths", self.paths.iter().collect::<ScubaValue>());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::CommitSparseProfileDeltaParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("other_commit", self.other_id.to_string());
        scuba.add(
            "param_paths",
            match &self.profiles {
                thrift::SparseProfiles::all_profiles(_) => {
                    ScubaValue::from("all sparse profiles".to_string())
                }
                thrift::SparseProfiles::profiles(profiles) => {
                    profiles.iter().collect::<ScubaValue>()
                }
                thrift::SparseProfiles::UnknownField(t) => {
                    ScubaValue::from(format!("unknown SparseProfiles type {}", t))
                }
            },
        );
    }
}

impl AddScubaParams for thrift::CommitSparseProfileSizeParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add(
            "param_paths",
            match &self.profiles {
                thrift::SparseProfiles::all_profiles(_) => {
                    ScubaValue::from("all sparse profiles".to_string())
                }
                thrift::SparseProfiles::profiles(profiles) => {
                    profiles.iter().collect::<ScubaValue>()
                }
                thrift::SparseProfiles::UnknownField(t) => {
                    ScubaValue::from(format!("unknown SparseProfiles type {}", t))
                }
            },
        );
    }
}

impl AddScubaParams for thrift::FileContentChunkParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_size", self.size);
    }
}

impl AddScubaParams for thrift::FileExistsParams {}

impl AddScubaParams for thrift::FileInfoParams {}

impl AddScubaParams for thrift::FileDiffParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("other_file", hex(&self.other_file_id));
        scuba.add("param_format", self.format.to_string());
        scuba.add("param_context", self.context);
    }
}

impl AddScubaParams for thrift::TreeExistsParams {}

impl AddScubaParams for thrift::TreeListParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_limit", self.limit);
    }
}

impl AddScubaParams for thrift::MegarepoAddTargetToken {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoAddBranchingTargetToken {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoChangeConfigToken {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoRemergeSourceToken {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoSyncChangesetToken {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoSyncChangesetParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_source_name", self.source_name.clone());
        scuba.add("param_megarepo_cs_id", hex(&self.cs_id));
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoRemergeSourceParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_source_name", self.source_name.clone());
        scuba.add("param_megarepo_cs_id", hex(&self.cs_id));
        scuba.add("param_megarepo_target_location", hex(&self.target_location));
        scuba.add("param_megarepo_message", self.message.clone());
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoChangeTargetConfigParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_version", self.new_version.clone());
        scuba.add("param_megarepo_message", self.message.clone());
        scuba.add("param_megarepo_target_location", hex(&self.target_location));
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoAddTargetParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add(
            "param_megarepo_version",
            self.config_with_new_target.version.clone(),
        );
        scuba.add("param_megarepo_message", self.message.clone());
        report_megarepo_target(&self.config_with_new_target.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoAddBranchingTargetParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_branching_point", hex(&self.branching_point));
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoAddConfigParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("param_megarepo_version", self.new_config.version.clone());
        report_megarepo_target(&self.new_config.target, scuba, Reported::Param);
    }
}

impl AddScubaParams for thrift::MegarepoReadConfigParams {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_megarepo_target(&self.target, scuba, Reported::Param);
    }
}
