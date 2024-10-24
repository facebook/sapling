/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use dag::delegate;
use dag::errors::NotFoundError;
use dag::ops::DagAlgorithm;
use dag::ops::DagPersistent;
use dag::ops::DagStrip;
use dag::ops::IdConvert;
use dag::Dag;
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use format_util::git_sha1_deserialize;
use format_util::git_sha1_serialize;
use format_util::hg_sha1_deserialize;
use format_util::hg_sha1_serialize;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use minibytes::Bytes;
use parking_lot::RwLock;
use storemodel::SerializationFormat;
use types::HgId;
use zstore::Id20;
use zstore::Zstore;

use crate::utils;
use crate::AppendCommits;
use crate::DescribeBackend;
use crate::GraphNode;
use crate::HgCommit;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StreamCommitText;
use crate::StripCommits;

/// Commits stored on disk, identified by SHA1.
pub struct OnDiskCommits {
    commits: Arc<RwLock<Zstore>>,
    pub(crate) commits_path: PathBuf,
    pub(crate) dag: Dag,
    pub(crate) dag_path: PathBuf,
    /// Whether to use Git's SHA1 or Hg's SHA1 format.
    pub(crate) format: SerializationFormat,
}

impl OnDiskCommits {
    pub fn new(dag_path: &Path, commits_path: &Path, format: SerializationFormat) -> Result<Self> {
        tracing::trace!(target: "commits::format", ?format);
        let result = Self {
            dag: Dag::open(dag_path)?,
            dag_path: dag_path.to_path_buf(),
            commits: Arc::new(RwLock::new(Zstore::open(commits_path)?)),
            commits_path: commits_path.to_path_buf(),
            format,
        };
        Ok(result)
    }

    /// Import another DAG. `main` specifies the main branch for commit graph
    /// optimization.
    pub async fn import_dag(&mut self, other: impl DagAlgorithm, main: Set) -> Result<()> {
        self.dag.import_and_flush(&other, main).await?;
        Ok(())
    }

    pub(crate) fn commit_data_store(&self) -> Arc<RwLock<Zstore>> {
        self.commits.clone()
    }

    pub(crate) fn format(&self) -> SerializationFormat {
        self.format
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
            let vertex = Vertex::copy_from(self.commits.write().insert(&text, &[])?.as_ref());
            if vertex != commit.vertex {
                return Err(crate::errors::hash_mismatch(&vertex, &commit.vertex));
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
        let mut zstore = self.commits.write();
        zstore.flush()?;
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
        let store = self.commits.read();
        get_commit_raw_text(&store, vertex, self.format())
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        ArcRwLockZstore(self.commits.clone(), self.format()).to_dyn_read_commit_text()
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }
}

#[derive(Clone)]
struct ArcRwLockZstore(Arc<RwLock<Zstore>>, SerializationFormat);

#[async_trait::async_trait]
impl ReadCommitText for ArcRwLockZstore {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let store = self.0.read();
        get_commit_raw_text(&store, vertex, self.1)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }

    fn format(&self) -> SerializationFormat {
        self.1
    }
}

fn get_commit_raw_text(
    store: &Zstore,
    vertex: &Vertex,
    format: SerializationFormat,
) -> Result<Option<Bytes>> {
    let id = Id20::from_slice(vertex.as_ref())?;
    match store.get(id)? {
        Some(bytes) => {
            let raw_text = match format {
                SerializationFormat::Hg => hg_sha1_deserialize(bytes.as_ref())?.0,
                SerializationFormat::Git => git_sha1_deserialize(bytes.as_ref())?.0,
            };
            Ok(Some(bytes.slice_to_bytes(raw_text)))
        }
        None => Ok(crate::revlog::get_hard_coded_commit_text(vertex)),
    }
}

impl StreamCommitText for OnDiskCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let zstore = Zstore::open(&self.commits_path)?;
        let stream = stream.map(move |item| {
            let vertex = item?;
            let id = Id20::from_slice(vertex.as_ref())?;
            // Mercurial hard-coded special-case that does not match SHA1.
            let raw_text = if id.is_null() || id.is_wdir() {
                Default::default()
            } else {
                match zstore.get(id)? {
                    Some(raw_data) => raw_data.slice(Id20::len() * 2..),
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
