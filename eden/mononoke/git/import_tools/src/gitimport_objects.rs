/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use git2::{ObjectType, Oid, Repository, Revwalk, Time};
use git_pool::GitPool;
use git_types::mode;
use manifest::{Entry, Manifest, StoreLoadable};
use mononoke_types::{hash, typed_hash::ChangesetId, DateTime, FileType, MPathElement};
use slog::debug;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

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
                        let ft = convert_git_filemode(filemode)?;

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

pub fn convert_git_filemode(git_filemode: i32) -> Result<FileType, Error> {
    match git_filemode {
        mode::GIT_FILEMODE_BLOB => Ok(FileType::Regular),
        mode::GIT_FILEMODE_BLOB_EXECUTABLE => Ok(FileType::Executable),
        mode::GIT_FILEMODE_LINK => Ok(FileType::Symlink),
        _ => Err(format_err!("Invalid filemode: {:?}", git_filemode)),
    }
}

#[async_trait]
impl StoreLoadable<GitPool> for GitTree {
    type Value = GitManifest;

    async fn load<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        pool: &'a GitPool,
    ) -> Result<Self::Value, LoadableError> {
        load_git_tree(self.0, pool)
            .await
            .map_err(LoadableError::from)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GitimportPreferences {
    pub derive_trees: bool,
    pub derive_hg: bool,
    pub hggit_compatibility: bool,
    pub bonsai_git_mapping: bool,
    pub gitrepo_name: Option<String>,
}

impl GitimportPreferences {
    pub fn enable_derive_trees(&mut self) {
        self.derive_trees = true
    }

    pub fn enable_derive_hg(&mut self) {
        self.derive_hg = true
    }

    pub fn enable_hggit_compatibility(&mut self) {
        self.hggit_compatibility = true
    }

    pub fn enable_bonsai_git_mapping(&mut self) {
        self.bonsai_git_mapping = true
    }

    /// Only for logging purpuses,
    /// useful when several repos are imported simultainously.
    pub fn set_gitrepo_name(&mut self, name: String) {
        self.gitrepo_name = Some(name);
    }
}

pub fn oid_to_sha1(oid: &Oid) -> Result<hash::GitSha1, Error> {
    hash::GitSha1::from_bytes(Bytes::copy_from_slice(oid.as_bytes()))
}

pub trait GitimportTarget {
    fn populate_walk(&self, repo: &Repository, walk: &mut Revwalk) -> Result<(), Error>;

    /// Roots are the Oid -> ChangesetId mappings that already are
    /// imported into Mononoke.
    fn get_roots(&self) -> Result<HashMap<Oid, ChangesetId>, Error>;

    fn get_nb_commits(&self, repo: &Repository) -> Result<usize, Error> {
        let mut walk = repo.revwalk()?;
        self.populate_walk(repo, &mut walk)?;
        Ok(walk.count())
    }
}

pub struct FullRepoImport {}

impl GitimportTarget for FullRepoImport {
    fn populate_walk(&self, repo: &Repository, walk: &mut Revwalk) -> Result<(), Error> {
        for reference in repo.references()? {
            let reference = reference?;
            if let Some(oid) = reference.target() {
                walk.push(oid)?;
            }
        }
        Ok(())
    }

    fn get_roots(&self) -> Result<HashMap<Oid, ChangesetId>, Error> {
        Ok(HashMap::new())
    }
}

pub struct GitRangeImport {
    pub from: Oid,
    pub from_csid: ChangesetId,
    pub to: Oid,
}

impl GitRangeImport {
    pub async fn new(
        from: Oid,
        to: Oid,
        ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<GitRangeImport, Error> {
        let from_csid = repo
            .bonsai_git_mapping()
            .get_bonsai_from_git_sha1(&ctx, hash::GitSha1::from_bytes(from)?)
            .await?
            .ok_or_else(|| {
                format_err!(
                    "Cannot start import from root {}: commit does not exist in Blobrepo",
                    from
                )
            })?;
        Ok(GitRangeImport {
            from,
            from_csid,
            to,
        })
    }
}

impl GitimportTarget for GitRangeImport {
    fn populate_walk(&self, _: &Repository, walk: &mut Revwalk) -> Result<(), Error> {
        walk.hide(self.from)?;
        walk.push(self.to)?;
        Ok(())
    }

    fn get_roots(&self) -> Result<HashMap<Oid, ChangesetId>, Error> {
        let mut roots = HashMap::new();
        roots.insert(self.from, self.from_csid);
        Ok(roots)
    }
}

/// Intended to import all git commits that are missing to fully
/// represent specified commit with all its histroy.
/// It will check what is already present and only import the minimum set required.
pub struct ImportMissingForCommit {
    commit: Oid,
    commits_to_add: usize,
    roots: HashMap<Oid, ChangesetId>,
}

impl ImportMissingForCommit {
    pub async fn new(
        commit: Oid,
        ctx: &CoreContext,
        repo: &BlobRepo,
        gitrepo: &Repository,
    ) -> Result<ImportMissingForCommit, Error> {
        let ta = Instant::now();

        // Starting from the specified commit. We need to get the boundaries of what already is imported into Mononoke.
        // We do this by doing a bfs search from the specified commit.
        let mut existing = HashMap::<Oid, ChangesetId>::new();
        let mut visisted = HashSet::new();
        let mut q = Vec::new();
        q.push(commit);
        while !q.is_empty() {
            let id = q.pop().unwrap();
            if !visisted.contains(&id) {
                visisted.insert(id);
                if let Some(changeset) =
                    ImportMissingForCommit::commit_in_mononoke(ctx, repo, &id).await?
                {
                    existing.insert(id, changeset);
                } else {
                    q.extend(gitrepo.find_commit(id)?.parent_ids());
                }
            }
        }

        let commits_to_add = visisted.len() - existing.len();

        let tb = Instant::now();
        debug!(
            ctx.logger(),
            "Time to find missing commits {:?}",
            tb.duration_since(ta)
        );

        Ok(ImportMissingForCommit {
            commit,
            commits_to_add,
            roots: existing,
        })
    }

    async fn commit_in_mononoke(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit_id: &Oid,
    ) -> Result<Option<ChangesetId>, Error> {
        let changeset = repo
            .bonsai_git_mapping()
            .get_bonsai_from_git_sha1(ctx, oid_to_sha1(commit_id)?)
            .await?;
        if let Some(existing_changeset) = changeset {
            debug!(
                ctx.logger(),
                "Commit found in Mononoke Oid:{} -> ChangesetId:{}",
                oid_to_sha1(commit_id)?.to_brief(),
                existing_changeset.to_brief()
            );
        }
        Ok(changeset)
    }
}

impl GitimportTarget for ImportMissingForCommit {
    fn populate_walk(&self, _: &Repository, walk: &mut Revwalk) -> Result<(), Error> {
        walk.push(self.commit)?;
        self.roots.keys().try_for_each(|v| walk.hide(*v))?;
        Ok(())
    }

    fn get_roots(&self) -> Result<HashMap<Oid, ChangesetId>, Error> {
        Ok(self.roots.clone())
    }

    fn get_nb_commits(&self, _: &Repository) -> Result<usize, Error> {
        Ok(self.commits_to_add)
    }
}

pub struct CommitMetadata {
    pub oid: Oid,
    pub parents: Vec<Oid>,
    pub message: String,
    pub author: String,
    pub author_date: DateTime,
    pub committer: String,
    pub committer_date: DateTime,
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

            let author = format!("{}", commit.author());
            let committer = format!("{}", commit.committer());

            let message = commit.message().unwrap_or_default().to_owned();

            let parents = commit.parents().map(|p| p.id()).collect();

            let time = commit.author().when();
            let author_date = convert_time_to_datetime(&time)?;
            let time = commit.committer().when();
            let committer_date = convert_time_to_datetime(&time)?;

            Result::<_, Error>::Ok(ExtractedCommit {
                metadata: CommitMetadata {
                    oid: commit.id(),
                    parents,
                    message,
                    author,
                    author_date,
                    committer,
                    committer_date,
                },
                tree,
                parent_trees,
            })
        })
        .await
    }
}

fn convert_time_to_datetime(time: &Time) -> Result<DateTime, Error> {
    DateTime::from_timestamp(time.seconds(), -1 * time.offset_minutes() * 60)
}
