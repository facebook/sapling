/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;

use dag::ops::DagAlgorithm;
use dag::ops::DagPersistent;
use dag::Dag;
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use nonblocking::non_blocking_result;
use parking_lot::Mutex;

use crate::errors::MapDagError;

/// `GitDag` maintains segmented changelog as an index on the Git commit graph.
///
/// This struct provides a "read-only" view for the commit graph. To read other
/// parts of the git repo, or make changes to the Git commit graph, use a
/// separate `git2::Repository` object.
///
/// The `dag` part is append-only. It might include vertexes no longer referred
/// by the git repo. Use `ancestors(git_heads())` to get commits referred by
/// the git repo, and use `&` to filter them.
pub struct GitDag {
    dag: Dag,
    heads: Set,
    references: BTreeMap<String, Vertex>,
}

impl GitDag {
    /// `open` a Git repo at `git_dir`. Build index at `dag_dir`, with specified `main_branch`.
    pub fn open(git_dir: &Path, dag_dir: &Path, main_branch: &str) -> dag::Result<Self> {
        let git_repo = git2::Repository::open(git_dir)
            .with_context(|| format!("opening git repo at {}", git_dir.display()))?;
        Self::open_git_repo(&git_repo, dag_dir, main_branch)
    }

    /// For an git repo, build index at `dag_dir` with specified `main_branch`.
    pub fn open_git_repo(
        git_repo: &git2::Repository,
        dag_dir: &Path,
        main_branch: &str,
    ) -> dag::Result<Self> {
        let dag = Dag::open(dag_dir)?;
        Ok(sync_from_git(dag, git_repo, main_branch)?)
    }

    /// Get "snapshotted" references.
    pub fn git_references(&self) -> &BTreeMap<String, Vertex> {
        &self.references
    }

    /// Get "snapshotted" heads.
    pub fn git_heads(&self) -> Set {
        self.heads.clone()
    }
}

impl Deref for GitDag {
    type Target = Dag;

    fn deref(&self) -> &Self::Target {
        &self.dag
    }
}

impl DerefMut for GitDag {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.dag
    }
}

/// Read references from git, build segments for new heads.
///
/// Useful when the git repo is changed by other processes or threads.
fn sync_from_git(
    mut dag: Dag,
    git_repo: &git2::Repository,
    main_branch: &str,
) -> dag::Result<GitDag> {
    let mut master_heads = Vec::new();
    let mut non_master_heads = Vec::new();
    let mut references = BTreeMap::new();

    let git_refs = git_repo.references().context("listing git references")?;
    for git_ref in git_refs {
        let git_ref = git_ref.context("resolving git reference")?;
        let commit = match git_ref.peel_to_commit() {
            Err(e) => {
                tracing::warn!(
                    "git ref {} cannot resolve to commit: {}",
                    String::from_utf8_lossy(git_ref.name_bytes()),
                    e
                );
                // Ignore this error. Some git references (ex. tags) can point
                // to trees instead of commits.
                continue;
            }
            Ok(c) => c,
        };
        let oid = commit.id();
        let vertex = Vertex::copy_from(oid.as_bytes());
        if let Some(name) = git_ref.name() {
            references.insert(name.to_string(), vertex.clone());
        }
        if git_ref.name() == Some(main_branch) {
            master_heads.push(vertex);
        } else {
            non_master_heads.push(vertex);
        }
    }

    struct ForceSend<T>(T);

    // See https://github.com/rust-lang/git2-rs/issues/194, libgit2 can be
    // accessed by a different thread.
    unsafe impl<T> Send for ForceSend<T> {}

    let git_repo = ForceSend(git_repo);
    let git_repo = Mutex::new(git_repo);

    let parent_func = move |v: Vertex| -> dag::Result<Vec<Vertex>> {
        tracing::trace!("visiting git commit {:?}", &v);
        let oid = git2::Oid::from_bytes(v.as_ref())
            .with_context(|| format!("converting to git oid for {:?}", &v))?;
        let commit = git_repo
            .lock()
            .0
            .find_commit(oid)
            .with_context(|| format!("resolving {:?} to git commit", &v))?;
        Ok(commit
            .parent_ids()
            .map(|id| Vertex::copy_from(id.as_bytes()))
            .collect())
    };
    let parents: Box<dyn Fn(Vertex) -> dag::Result<Vec<Vertex>> + Send + Sync> =
        Box::new(parent_func);
    let heads = VertexListWithOptions::from(master_heads.clone())
        .with_highest_group(Group::MASTER)
        .chain(non_master_heads.clone());
    non_blocking_result(dag.add_heads_and_flush(&parents, &heads))?;

    let possible_heads =
        Set::from_static_names(master_heads.into_iter().chain(non_master_heads.into_iter()));
    let heads = non_blocking_result(dag.heads_ancestors(possible_heads))?;

    Ok(GitDag {
        dag,
        heads,
        references,
    })
}
