/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_api::RepoContext;

use super::HgRepoContext;

pub trait RepoContextHgExt {
    /// Get an HgRepoContext to access this repo's data in Mercurial-specific formats.
    fn hg(self) -> HgRepoContext;
}

impl RepoContextHgExt for RepoContext {
    fn hg(self) -> HgRepoContext {
        HgRepoContext::new(self)
    }
}
