/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use crate::git_pool::GitPool;
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::LoadableError;
use context::CoreContext;
use futures::future::{BoxFuture, FutureExt as _};
use git2::{ObjectType, Oid, Repository, Revwalk};
use git_types::mode;
use manifest::{Entry, Manifest, StoreLoadable};
use mononoke_types::{hash::GitSha1, typed_hash::ChangesetId, DateTime, FileType, MPathElement};
use std::collections::{HashMap, HashSet};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitTree(pub Oid);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitLeaf(pub Oid);

pub struct GitManifest(HashMap<MPathElement, Entry<GitTree, (FileType, GitLeaf)>>);

impl Manifest for GitManifest {
    type TreeId = GitTree;
    type LeafId = (FileType, GitLeaf);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(self.0.clone().into_iter())
    }
}

async fn load_git_tree(oid: Oid, pool: &GitPool) -> Result<GitManifest, Error> {
    pool.with(move |repo| {
        let tree = repo.find_tree(oid)?;

        let elements = tree
            .iter()
            .map(|entry| {
                let oid = entry.id();
                let filemode = entry.filemode();
                let name = MPathElement::new(entry.name_bytes().into())?;

                let r = match entry.kind() {
                    Some(ObjectType::Blob) => {
                        let ft = match filemode {
                            mode::GIT_FILEMODE_BLOB => FileType::Regular,
                            mode::GIT_FILEMODE_BLOB_EXECUTABLE => FileType::Executable,
                            mode::GIT_FILEMODE_LINK => FileType::Symlink,
                            _ => {
                                return Err(format_err!("Invalid filemode: {:?}", filemode));
                            }
                        };

                        (name, Entry::Leaf((ft, GitLeaf(oid))))
                    }
                    Some(ObjectType::Tree) => (name, Entry::Tree(GitTree(oid))),
                    k => {
                        return Err(format_err!("Invalid kind: {:?}", k));
                    }
                };

                Ok(r)
            })
            .collect::<Result<HashMap<_, _>, Error>>()?;

        Result::<_, Error>::Ok(GitManifest(elements))
    })
    .await
}

impl StoreLoadable<GitPool> for GitTree {
    type Value = GitManifest;

    fn load(
        &self,
        _ctx: CoreContext,
        pool: &GitPool,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>> {
        let oid = self.0;
        let pool = pool.clone();
        async move { load_git_tree(oid, &pool).await.map_err(LoadableError::from) }.boxed()
    }
}

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

pub struct CommitMetadata {
    pub oid: Oid,
    pub parents: Vec<Oid>,
    pub author: String,
    pub message: String,
    pub author_date: DateTime,
}

pub struct ExtractedCommit {
    pub metadata: CommitMetadata,
    pub tree: GitTree,
    pub parent_trees: HashSet<GitTree>,
}

impl ExtractedCommit {
    pub async fn new(oid: Oid, pool: &GitPool) -> Result<Self, Error> {
        pool.with(move |repo| {
            let commit = repo.find_commit(oid)?;

            let tree = GitTree(commit.tree()?.id());

            let parent_trees = commit
                .parents()
                .map(|p| {
                    let tree = p.tree()?;
                    Ok(GitTree(tree.id()))
                })
                .collect::<Result<_, Error>>()?;

            // TODO: Include email in the author
            let author = commit
                .author()
                .name()
                .ok_or_else(|| format_err!("Commit has no author: {:?}", commit.id()))?
                .to_owned();

            let message = commit.message().unwrap_or_default().to_owned();

            let parents = commit.parents().map(|p| p.id()).collect();

            let time = commit.time();
            let author_date = DateTime::from_timestamp(time.seconds(), time.offset_minutes() * 60)?;

            Result::<_, Error>::Ok(ExtractedCommit {
                metadata: CommitMetadata {
                    oid: commit.id(),
                    parents,
                    message,
                    author,
                    author_date,
                },
                tree,
                parent_trees,
            })
        })
        .await
    }
}
