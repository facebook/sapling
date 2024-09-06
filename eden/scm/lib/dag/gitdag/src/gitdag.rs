/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;

use dag::ops::DagPersistent;
use dag::Dag;
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
    git_dir: PathBuf,
}

impl GitDag {
    /// Creates `GitDag`. This does not automatically import Git references.
    /// The callsite is expected to read, resolve Git references, then call
    /// `sync_from_git` to import them.
    pub fn open(dag_dir: &Path, git_dir: &Path) -> dag::Result<Self> {
        let dag = Dag::open(dag_dir)?;
        let git_dir = git_dir.to_owned();
        Ok(Self { dag, git_dir })
    }

    /// Import heads (and ancestors) from Git objects to the `dag`.
    /// The commit hashes are imported, but not the commit messages.
    pub fn import_from_git(
        &mut self,
        git_repo: Option<&git2::Repository>,
        heads: VertexListWithOptions,
    ) -> anyhow::Result<()> {
        if heads.is_empty() {
            return Ok(());
        }
        // git_repo is used to read local objects, not for reading references.
        let git_repo_owned;
        let git_repo_ref = match git_repo {
            None => {
                git_repo_owned = git2::Repository::open(&self.git_dir)
                    .with_context(|| format!("opening git repo at {}", self.git_dir.display()))?;
                &git_repo_owned
            }
            Some(repo) => repo,
        };
        sync_from_git(&mut self.dag, git_repo_ref, heads)?;
        Ok(())
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

/// Read from Git commit objects. Build segments for provided heads.
fn sync_from_git(
    dag: &mut Dag,
    git_repo: &git2::Repository,
    heads: VertexListWithOptions,
) -> dag::Result<()> {
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

    non_blocking_result(dag.add_heads_and_flush(&parents, &heads))?;

    Ok(())
}
