/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for repository selection.

use source_control::types as thrift;

#[derive(clap::Args, Clone)]
pub(crate) struct RepoArgs {
    #[clap(long, short = 'R')]
    /// Repository name
    repo: String,
}

impl RepoArgs {
    pub fn into_repo_specifier(self) -> thrift::RepoSpecifier {
        thrift::RepoSpecifier {
            name: self.repo,
            ..Default::default()
        }
    }
}
