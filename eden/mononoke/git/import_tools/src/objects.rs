/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use context::CoreContext;
use git2::{Oid, Repository, Revwalk};
use mononoke_types::{hash::GitSha1, typed_hash::ChangesetId};
use std::collections::HashMap;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct GitimportPreferences {
    pub dry_run: bool,
    pub derive_trees: bool,
    pub derive_hg: bool,
    pub hggit_compatibility: bool,
}

impl GitimportPreferences {
    pub fn enable_dry_run(&mut self) {
        self.dry_run = true
    }

    pub fn enable_derive_trees(&mut self) {
        self.derive_trees = true
    }

    pub fn enable_derive_hg(&mut self) {
        self.derive_hg = true
    }

    pub fn enable_hggit_compatibility(&mut self) {
        self.hggit_compatibility = true
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum GitimportTarget {
    FullRepo,
    GitRange(Oid, Oid),
}

impl GitimportTarget {
    pub fn populate_walk(&self, repo: &Repository, walk: &mut Revwalk) -> Result<(), Error> {
        match self {
            Self::FullRepo => {
                for reference in repo.references()? {
                    let reference = reference?;
                    if let Some(oid) = reference.target() {
                        walk.push(oid)?;
                    }
                }
            }
            Self::GitRange(from, to) => {
                walk.hide(*from)?;
                walk.push(*to)?;
            }
        };

        Ok(())
    }

    pub async fn populate_roots(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        roots: &mut HashMap<Oid, ChangesetId>,
    ) -> Result<(), Error> {
        match self {
            Self::FullRepo => {
                // Noop
            }
            Self::GitRange(from, _to) => {
                let root = repo
                    .bonsai_git_mapping()
                    .get_bonsai_from_git_sha1(&ctx, GitSha1::from_bytes(from)?)
                    .await?
                    .ok_or_else(|| {
                        format_err!(
                            "Cannot start import from {}: commit does not exist in Blobrepo",
                            from
                        )
                    })?;

                roots.insert(*from, root);
            }
        };

        Ok(())
    }
}
