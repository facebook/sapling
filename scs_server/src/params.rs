/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
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
pub(crate) trait AddScubaParams {
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

impl AddScubaParams for thrift::RepoCreateCommitParams {}

impl AddScubaParams for thrift::RepoListBookmarksParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_include_scratch", self.include_scratch as i32);
        scuba.add("param_bookmark_prefix", self.bookmark_prefix.as_str());
        scuba.add("param_limit", self.limit);
        self.identity_schemes.add_scuba_params(scuba);
    }
}

impl AddScubaParams for thrift::RepoResolveBookmarkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark_name.as_str());
        self.identity_schemes.add_scuba_params(scuba);
    }
}

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

impl AddScubaParams for thrift::CommitLookupParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
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
        // Blame only supports a single identity scheme, but
        // still log this as a normvector for consistency.
        scuba.add(
            "identity_schemes",
            Some(self.identity_scheme.to_string())
                .iter()
                .collect::<ScubaValue>(),
        );
    }
}

impl AddScubaParams for thrift::CommitPathInfoParams {}

impl AddScubaParams for thrift::FileContentChunkParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_size", self.size);
    }
}

impl AddScubaParams for thrift::FileExistsParams {}

impl AddScubaParams for thrift::FileInfoParams {}

impl AddScubaParams for thrift::TreeListParams {
    fn add_scuba_params(&self, scuba: &mut ScubaSampleBuilder) {
        scuba.add("param_offset", self.offset);
        scuba.add("param_limit", self.limit);
    }
}
