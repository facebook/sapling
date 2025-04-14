/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;

use dag::ops::DagPersistent;
use dag::Dag;
use dag::Vertex;
use dag::VertexListWithOptions;
use gitstore::GitStore;
use gitstore::ObjectType;
use minibytes::Bytes;
use nonblocking::non_blocking_result;
use types::fetch_mode::FetchMode;
use types::HgId;
use types::SerializationFormat;

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
}

impl GitDag {
    /// Creates `GitDag`. This does not automatically import Git references.
    /// The callsite is expected to read, resolve Git references, then call
    /// `sync_from_git` to import them.
    pub fn open(dag_dir: &Path) -> dag::Result<Self> {
        let dag = Dag::open(dag_dir)?;
        Ok(Self { dag })
    }

    /// Import heads (and ancestors) from Git objects to the `dag`.
    /// The commit hashes are imported, but not the commit messages.
    pub fn import_from_git(
        &mut self,
        git_store: &GitStore,
        heads: VertexListWithOptions,
    ) -> anyhow::Result<()> {
        if heads.is_empty() {
            return Ok(());
        }
        // git_repo is used to read local objects, not for reading references.
        sync_from_git(&mut self.dag, git_store, heads)?;
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
    git_store: &GitStore,
    heads: VertexListWithOptions,
) -> anyhow::Result<()> {
    // Filter out non-commit (ex. tree) references.
    let heads = heads.try_filter(&|vertex, _opts| {
        let id = HgId::from_slice(vertex.as_ref())?;
        // `has_obj` is not enough. We need to filter out objects of wrong type (ex. trees) too.
        // Sapling's references cannot be "tree"s.
        Ok(git_store
            .read_local_obj_optional(id, ObjectType::Commit)?
            .is_some())
    })?;

    let git_store = git_store.clone();
    let parent_func = move |v: Vertex| -> dag::Result<Vec<Vertex>> {
        tracing::trace!("visiting git commit {:?}", &v);
        let id = HgId::from_slice(v.as_ref())
            .map_err(anyhow::Error::from)
            .context("converting to SHA1")?;
        let bytes = git_store
            .read_obj(id, ObjectType::Commit, FetchMode::LocalOnly)
            .context("reading git commit")?;
        let bytes: Bytes = bytes.into();
        let text = bytes.into_text_lossy();
        let fields = format_util::commit_text_to_fields(text, SerializationFormat::Git);
        let parents = fields
            .parents()
            .context("extracting parents from git commit")?;
        let parents = parents.unwrap_or(&[]);
        Ok(parents
            .iter()
            .map(|id| Vertex::copy_from(id.as_ref()))
            .collect())
    };
    let parents: Box<dyn Fn(Vertex) -> dag::Result<Vec<Vertex>> + Send + Sync> =
        Box::new(parent_func);

    non_blocking_result(dag.add_heads_and_flush(&parents, &heads))?;

    Ok(())
}
