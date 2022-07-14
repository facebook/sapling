/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use itertools::Itertools;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::permissions::WritePermissionsModel;
use crate::repo_draft::RepoDraftContext;

use std::collections::HashMap;

const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
const HGGIT_MARKER_VALUE: &[u8] = b"git";
const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

impl RepoDraftContext {
    /// Set the bonsai to git mapping based on the changeset
    /// If the user is trusted, this will use the hggit extra
    /// Otherwise, it will only work if we can derive a git commit ID, and that ID matches the hggit extra
    /// or the hggit extra is missing from the changeset completely.
    pub async fn set_git_mapping_from_changeset(
        &self,
        changeset_ctx: &ChangesetContext,
    ) -> Result<(), MononokeError> {
        let mut extras: HashMap<_, _> = changeset_ctx.extras().await?.into_iter().collect();

        //TODO(simonfar): Once we support deriving git commits, do derivation here
        // If there's no hggit extras, then give back the derived hash.
        // If there's a hggit extra, and it matches the derived commit, accept even if you
        // don't have permission

        if extras.get(HGGIT_MARKER_EXTRA).map(Vec::as_slice) == Some(HGGIT_MARKER_VALUE) {
            if let Some(hggit_sha1) = extras.remove(HGGIT_COMMIT_ID_EXTRA) {
                // We can't derive right now, so always do the permission check
                self.check_method_permitted("set_git_mapping_from_changeset")?;
                // TODO(simonfar): Relax this to a hipster group check when @mbthomas lands repo_authorization rework
                if let WritePermissionsModel::AllowAnyWrite = self.permissions_model {
                    let identities = self.repo.ctx().metadata().identities();
                    let identities = if identities.is_empty() {
                        "<none>".to_string()
                    } else {
                        identities.iter().join(",")
                    };
                    return Err(MononokeError::PermissionDenied {
                        mode: "write git mapping".to_string(),
                        identities,
                        reponame: self.repo.name().to_string(),
                    });
                }

                let hggit_sha1 = String::from_utf8_lossy(&hggit_sha1).parse()?;
                let entry = BonsaiGitMappingEntry::new(hggit_sha1, changeset_ctx.id());
                let mapping = self.repo.inner_repo().bonsai_git_mapping();
                mapping
                    .bulk_add(self.repo.ctx(), &[entry])
                    .await
                    .context("set_git_mapping_from_changeset")?;
            }
        }

        Ok(())
    }
}
