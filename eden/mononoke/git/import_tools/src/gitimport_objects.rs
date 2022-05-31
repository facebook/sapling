/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::gitlfs::GitImportLfs;
use crate::{git2_oid_to_git_hash_objectid, git_hash_oid_to_git2_oid};

use anyhow::{bail, format_err, Error};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::LoadableError;
use context::CoreContext;
use git2::{ObjectType, Repository, Revwalk};
use git_hash::ObjectId;
use git_object::{tree, Commit, CommitRef, Tree, TreeRef};
use git_pool::GitPool;
use manifest::{Entry, Manifest, StoreLoadable};
use mononoke_types::{hash, typed_hash::ChangesetId, DateTime, FileType, MPathElement};
use slog::debug;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitTree(pub ObjectId);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitLeaf(pub ObjectId);

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

pub(crate) fn read_tree(repo: &Repository, id: ObjectId) -> Result<Tree, Error> {
    let odb = repo.odb()?;
    let odb_object = odb.read(git_hash_oid_to_git2_oid(&id))?;
    if odb_object.kind() != ObjectType::Tree {
        bail!("{} is not a tree", id);
    }
    let tree_ref = TreeRef::from_bytes(odb_object.data())?;
    Ok(tree_ref.into())
}

async fn load_git_tree(oid: ObjectId, pool: &GitPool) -> Result<GitManifest, Error> {
    pool.with(move |repo| {
        let tree = read_tree(repo, oid)?;

        let elements = tree
            .entries
            .into_iter()
            .filter_map(
                |tree::Entry {
                     mode,
                     filename,
                     oid,
                 }| {
                    let name = match MPathElement::new(filename.into()) {
                        Ok(name) => name,
                        Err(e) => return Some(Err(e)),
                    };

                    let r = match mode {
                        tree::EntryMode::Blob => {
                            Some((name, Entry::Leaf((FileType::Regular, GitLeaf(oid)))))
                        }
                        tree::EntryMode::BlobExecutable => {
                            Some((name, Entry::Leaf((FileType::Executable, GitLeaf(oid)))))
                        }
                        tree::EntryMode::Link => {
                            Some((name, Entry::Leaf((FileType::Symlink, GitLeaf(oid)))))
                        }
                        tree::EntryMode::Tree => Some((name, Entry::Tree(GitTree(oid)))),

                        // git-sub-modules are represented as ObjectType::Commit inside the tree.
                        // For now we do not support git-sub-modules but we still need to import
                        // repositories that has sub-modules in them (just not synchronized), so
                        // ignoring any sub-module for now.
                        tree::EntryMode::Commit => None,
                    };
                    anyhow::Ok(r).transpose()
                },
            )
            .collect::<Result<_, Error>>()?;

        anyhow::Ok(GitManifest(elements))
    })
    .await
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

#[derive(Clone, Debug)]
pub struct GitimportPreferences {
    pub derive_trees: bool,
    pub derive_hg: bool,
    pub hggit_compatibility: bool,
    pub bonsai_git_mapping: bool,
    /// Only for logging purpuses,
    /// useful when several repos are imported simultainously.
    pub gitrepo_name: Option<String>,
    pub concurrency: usize,
    pub lfs: GitImportLfs,
}

impl Default for GitimportPreferences {
    fn default() -> Self {
        GitimportPreferences {
            derive_trees: false,
            derive_hg: false,
            hggit_compatibility: false,
            bonsai_git_mapping: false,
            gitrepo_name: None,
            concurrency: 20,
            lfs: GitImportLfs::default(),
        }
    }
}

pub fn oid_to_sha1(oid: &git_hash::oid) -> Result<hash::GitSha1, Error> {
    hash::GitSha1::from_bytes(oid.as_bytes())
}

pub trait GitimportTarget {
    fn populate_walk(&self, repo: &Repository, walk: &mut Revwalk) -> Result<(), Error>;

    /// Roots are the Oid -> ChangesetId mappings that already are
    /// imported into Mononoke.
    fn get_roots(&self) -> Result<HashMap<ObjectId, ChangesetId>, Error>;

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

    fn get_roots(&self) -> Result<HashMap<ObjectId, ChangesetId>, Error> {
        Ok(HashMap::new())
    }
}

pub struct GitRangeImport {
    pub from: ObjectId,
    pub from_csid: ChangesetId,
    pub to: ObjectId,
}

impl GitRangeImport {
    pub async fn new(
        from: ObjectId,
        to: ObjectId,
        ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<GitRangeImport, Error> {
        let from_csid = repo
            .bonsai_git_mapping()
            .get_bonsai_from_git_sha1(ctx, hash::GitSha1::from_bytes(from.as_bytes())?)
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
        walk.hide(git_hash_oid_to_git2_oid(&self.from))?;
        walk.push(git_hash_oid_to_git2_oid(&self.to))?;
        Ok(())
    }

    fn get_roots(&self) -> Result<HashMap<ObjectId, ChangesetId>, Error> {
        let mut roots = HashMap::new();
        roots.insert(self.from, self.from_csid);
        Ok(roots)
    }
}

/// Intended to import all git commits that are missing to fully
/// represent specified commit with all its history.
/// It will check what is already present and only import the minimum set required.
pub struct ImportMissingForCommit {
    commit: ObjectId,
    commits_to_add: usize,
    roots: HashMap<ObjectId, ChangesetId>,
}

impl ImportMissingForCommit {
    pub async fn new(
        commit: ObjectId,
        ctx: &CoreContext,
        repo: &BlobRepo,
        gitrepo: &Repository,
    ) -> Result<ImportMissingForCommit, Error> {
        let ta = Instant::now();

        // Starting from the specified commit. We need to get the boundaries of what already is imported into Mononoke.
        // We do this by doing a bfs search from the specified commit.
        let mut existing = HashMap::<ObjectId, ChangesetId>::new();
        let mut visited = HashSet::new();
        let mut q = Vec::new();
        q.push(commit);
        while !q.is_empty() {
            let id = q.pop().unwrap();
            if !visited.contains(&id) {
                visited.insert(id);
                if let Some(changeset) =
                    ImportMissingForCommit::commit_in_mononoke(ctx, repo, &id).await?
                {
                    existing.insert(id, changeset);
                } else {
                    let id = git_hash_oid_to_git2_oid(&id);
                    q.extend(
                        gitrepo
                            .find_commit(id)?
                            .parent_ids()
                            .map(|oid| git2_oid_to_git_hash_objectid(&oid)),
                    );
                }
            }
        }

        let commits_to_add = visited.len() - existing.len();

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
        commit_id: &git_hash::oid,
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
        walk.push(git_hash_oid_to_git2_oid(&self.commit))?;
        self.roots
            .keys()
            .try_for_each(|v| walk.hide(git_hash_oid_to_git2_oid(v)))?;
        Ok(())
    }

    fn get_roots(&self) -> Result<HashMap<ObjectId, ChangesetId>, Error> {
        Ok(self.roots.clone())
    }

    fn get_nb_commits(&self, _: &Repository) -> Result<usize, Error> {
        Ok(self.commits_to_add)
    }
}

pub struct CommitMetadata {
    pub oid: ObjectId,
    pub parents: Vec<ObjectId>,
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

pub(crate) fn read_commit(repo: &Repository, oid: &git_hash::oid) -> Result<Commit, Error> {
    let odb = repo.odb()?;
    let odb_object = odb.read(git_hash_oid_to_git2_oid(oid))?;
    if odb_object.kind() != ObjectType::Commit {
        bail!("{} is not a commit", oid);
    }
    let commit_ref = CommitRef::from_bytes(odb_object.data())?;
    Ok(commit_ref.into())
}

fn format_signature(sig: git_actor::SignatureRef) -> String {
    format!("{} <{}>", sig.name, sig.email)
}

impl ExtractedCommit {
    pub async fn new(oid: ObjectId, pool: &GitPool) -> Result<Self, Error> {
        pool.with(move |repo| {
            let Commit {
                tree,
                parents,
                author,
                committer,
                encoding,
                message,
                ..
            } = read_commit(repo, &oid)?;

            let tree = GitTree(tree);

            let parent_trees = {
                let mut trees = HashSet::new();
                for parent in &parents {
                    let commit = read_commit(repo, parent)?;
                    trees.insert(GitTree(commit.tree));
                }
                trees
            };

            let author_date = convert_time_to_datetime(&author.time)?;
            let committer_date = convert_time_to_datetime(&committer.time)?;

            if encoding.map_or(false, |bs| bs.to_ascii_lowercase() != b"utf-8") {
                bail!("Do not know how to handle non-UTF8")
            }

            let author = format_signature(author.to_ref());
            let committer = format_signature(committer.to_ref());

            let message = String::from_utf8(message.to_vec())?;

            let parents = parents.into_vec();

            Result::<_, Error>::Ok(ExtractedCommit {
                metadata: CommitMetadata {
                    oid,
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

pub fn convert_time_to_datetime(time: &git_actor::Time) -> Result<DateTime, Error> {
    DateTime::from_timestamp(
        time.seconds_since_unix_epoch.into(),
        -time.offset_in_seconds,
    )
}
