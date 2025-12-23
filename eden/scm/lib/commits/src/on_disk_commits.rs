/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use dag::Dag;
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::delegate;
use dag::errors::NotFoundError;
use dag::ops::DagAlgorithm;
use dag::ops::DagPersistent;
use dag::ops::DagStrip;
use dag::ops::IdConvert;
use eagerepo_trait::EagerRepoExtension;
use format_util::git_sha1_serialize;
use format_util::hg_sha1_serialize;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use id20store::Id20Store;
use minibytes::Bytes;
use storemodel::SerializationFormat;
use types::HgId;
use types::Id20;

use crate::AppendCommits;
use crate::DescribeBackend;
use crate::GraphNode;
use crate::HgCommit;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StreamCommitText;
use crate::StripCommits;
use crate::utils;

/// Commits stored on disk, identified by SHA1.
pub struct OnDiskCommits {
    commits: Arc<Id20Store>,
    pub(crate) commits_path: PathBuf,
    pub(crate) dag: Dag,
    pub(crate) dag_path: PathBuf,
    /// Whether to use Git's SHA1 or Hg's SHA1 format.
    pub(crate) format: SerializationFormat,
    /// Invalid commit hashes are present. Skip validating commit hashes on write.
    pub(crate) has_invalid_commit_hash: bool,
}

impl OnDiskCommits {
    pub fn new(dag_path: &Path, commits_path: &Path, format: SerializationFormat) -> Result<Self> {
        tracing::trace!(target: "commits::format", ?format);
        let store = Id20Store::open(commits_path, format)?;
        let mut dag = Dag::open(dag_path)?;

        // Load EagerRepo-compatible extension that can provide hash<->location commit hash
        // translation for virtual-repo.
        if let Some(name) = store.ext_name() {
            let ext = factory::call_constructor::<_, Arc<dyn EagerRepoExtension>>(&(
                name.to_string(),
                format,
            ))?;
            if let Some(remote_protocol) = ext.get_dag_remote_protocol() {
                dag.set_remote_protocol(remote_protocol);
            }
        }

        let result = Self {
            dag,
            dag_path: dag_path.to_path_buf(),
            commits: Arc::new(store),
            commits_path: commits_path.to_path_buf(),
            format,
            has_invalid_commit_hash: false,
        };
        Ok(result)
    }

    /// Sets the `has_invalid_commit_hash` field.
    /// If true, skip commit hash validation during commit writes.
    pub fn with_invalid_commit_hash(mut self, value: bool) -> Self {
        self.has_invalid_commit_hash = value;
        self
    }

    /// Import another DAG. `main` specifies the main branch for commit graph
    /// optimization.
    pub async fn import_dag(&mut self, other: impl DagAlgorithm, main: Set) -> Result<()> {
        self.dag.import_and_flush(&other, main).await?;
        Ok(())
    }

    pub(crate) fn commit_data_store(&self) -> Arc<Id20Store> {
        self.commits.clone()
    }
}

#[async_trait::async_trait]
impl AppendCommits for OnDiskCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        // Construct the SHA1 raw text.
        // The SHA1 of the returned value should match the commit hash.
        let get_sha1_raw_text = match self.format() {
            SerializationFormat::Git => git_sha1_raw_text,
            SerializationFormat::Hg => hg_sha1_raw_text,
        };

        // Write commit data to zstore.
        for commit in commits {
            let text = get_sha1_raw_text(&commit.raw_text, &commit.parents)?;
            if self.has_invalid_commit_hash {
                let id20 = Id20::from_slice(commit.vertex.as_ref())?;
                self.commits.add_arbitrary_blob(id20, &text)?;
            } else {
                let vertex = Vertex::copy_from(self.commits.add_sha1_blob(&text, &[])?.as_ref());
                if vertex != commit.vertex {
                    return Err(crate::errors::hash_mismatch(&vertex, &commit.vertex));
                }
            }
        }

        // Write commit graph to DAG.
        let graph_nodes = utils::commits_to_graph_nodes(commits);
        self.add_graph_nodes(&graph_nodes).await?;

        Ok(())
    }

    async fn add_graph_nodes(&mut self, graph_nodes: &[GraphNode]) -> Result<()> {
        utils::add_graph_nodes_to_dag(&mut self.dag, graph_nodes).await
    }

    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        self.flush_commit_data().await?;
        let heads = VertexListWithOptions::from(master_heads).with_desired_group(Group::MASTER);
        self.dag.flush(&heads).await?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        self.commits.flush()?;
        Ok(())
    }

    async fn update_virtual_nodes(&mut self, wdir_parents: Vec<Vertex>) -> Result<()> {
        // For hg compatibility, use the same hardcoded hashes.
        let null = Vertex::from(HgId::null_id().as_ref());
        let wdir = Vertex::from(HgId::wdir_id().as_ref());
        tracing::trace!("update wdir parents: {:?}", &wdir_parents);
        let items = vec![(null.clone(), Vec::new()), (wdir.clone(), wdir_parents)];
        self.dag.set_managed_virtual_group(Some(items)).await?;
        let null_rev = self.dag.vertex_id(null).await?;
        let wdir_rev = self.dag.vertex_id(wdir).await?;
        if Group::VIRTUAL.min_id() != null_rev {
            bail!("unexpected null rev: {:?}", null_rev);
        }
        if Group::VIRTUAL.min_id() + 1 != wdir_rev {
            bail!("unexpected wdir rev: {:?}", wdir_rev);
        }
        tracing::trace!(null_rev=?null_rev, wdir_rev=?wdir_rev, dag_version=?self.dag.dag_version(), "updated virtual revs");
        Ok(())
    }
}

fn hg_sha1_raw_text(raw_text: &[u8], parents: &[Vertex]) -> Result<Vec<u8>> {
    let p1 = match parents.first() {
        Some(v) => Id20::from_slice(v.as_ref())?,
        None => *Id20::null_id(),
    };
    let p2 = match parents.get(1) {
        Some(v) => Id20::from_slice(v.as_ref())?,
        None => *Id20::null_id(),
    };
    Ok(hg_sha1_serialize(raw_text, &p1, &p2))
}

fn git_sha1_raw_text(raw_text: &[u8], _parents: &[Vertex]) -> Result<Vec<u8>> {
    Ok(git_sha1_serialize(raw_text, "commit"))
}

#[async_trait::async_trait]
impl ReadCommitText for OnDiskCommits {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let store = &self.commits;
        get_commit_raw_text(store, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        ArcRwLockZstore(self.commits.clone(), self.format()).to_dyn_read_commit_text()
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }
}

#[derive(Clone)]
struct ArcRwLockZstore(Arc<Id20Store>, SerializationFormat);

#[async_trait::async_trait]
impl ReadCommitText for ArcRwLockZstore {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let store = &self.0;
        get_commit_raw_text(store, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }

    fn format(&self) -> SerializationFormat {
        self.1
    }
}

fn get_commit_raw_text(store: &Id20Store, vertex: &Vertex) -> Result<Option<Bytes>> {
    let id = Id20::from_slice(vertex.as_ref())?;
    match store.get_content(id)? {
        Some(v) => Ok(Some(v)),
        None => Ok(crate::revlog::get_hard_coded_commit_text(vertex)),
    }
}

impl StreamCommitText for OnDiskCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let format = self.format;
        let store = Id20Store::open(&self.commits_path, format)?;
        let stream = stream.map(move |item| {
            let vertex = item?;
            let id = Id20::from_slice(vertex.as_ref())?;
            // Mercurial hard-coded special-case that does not match SHA1.
            let raw_text = if id.is_null() || id.is_wdir() {
                Default::default()
            } else {
                match store.get_content(id)? {
                    Some(data) => data,
                    None => return vertex.not_found().map_err(Into::into),
                }
            };
            Ok(ParentlessHgCommit { vertex, raw_text })
        });
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl StripCommits for OnDiskCommits {
    async fn strip_commits(&mut self, set: Set) -> Result<()> {
        self.dag.strip(&set).await.map_err(Into::into)
    }
}

delegate!(CheckIntegrity | IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, OnDiskCommits => self.dag);

impl DescribeBackend for OnDiskCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (non-lazy segments):
  Local:
    Segments + IdMap: {}
    Zstore: {}
Feature Providers:
  Commit Graph Algorithms:
    Segments
  Commit Hash / Rev Lookup:
    IdMap
  Commit Data (user, message):
    Zstore
"#,
            self.dag_path.display(),
            self.commits_path.display()
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        write!(w, "{:?}", &self.dag)
    }
}

#[cfg(test)]
mod tests {
    use nonblocking::non_blocking_result as r;

    use super::*;

    #[test]
    fn test_hg_virtual_commits_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let commits = OnDiskCommits::new(
            &path.join("dag"),
            &path.join("commits"),
            SerializationFormat::Hg,
        )
        .unwrap();

        let wdir_node = Vertex::copy_from(Id20::wdir_id().as_ref());
        assert!(
            r(commits.get_commit_raw_text(&wdir_node))
                .unwrap()
                .is_some()
        );
        let null_node = Vertex::copy_from(Id20::null_id().as_ref());
        assert!(
            r(commits.get_commit_raw_text(&null_node))
                .unwrap()
                .is_some()
        );
    }
}
