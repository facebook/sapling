/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

use async_trait::async_trait;
use dag::delegate;
use dag::ops::DagAlgorithm;
use dag::ops::DagImportCloneData;
use dag::ops::DagImportPullData;
use dag::ops::DagPersistent;
use dag::protocol::AncestorPath;
use dag::protocol::RemoteIdConvertProtocol;
use dag::CloneData;
use dag::Location;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use edenapi::types::CommitLocationToHashRequest;
use edenapi::EdenApi;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use minibytes::Bytes;
use parking_lot::RwLock;
use streams::HybridResolver;
use streams::HybridStream;
use tracing::instrument;
use zstore::Id20;
use zstore::Zstore;

use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::HgCommits;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::RevlogCommits;
use crate::StreamCommitText;
use crate::StripCommits;

/// Segmented Changelog + Revlog (Optional) + Remote.
///
/// Use segmented changelog for the commit graph algorithms and IdMap.
/// Optionally writes to revlog just for fallback.
///
/// Use edenapi to resolve public commit messages and hashes.
pub struct HybridCommits {
    revlog: Option<RevlogCommits>,
    commits: HgCommits,
    client: Arc<dyn EdenApi>,
    lazy_hash_desc: String,
}

const EDENSCM_DISABLE_REMOTE_RESOLVE: &str = "EDENSCM_DISABLE_REMOTE_RESOLVE";
const EDENSCM_REMOTE_ID_THRESHOLD: &str = "EDENSCM_REMOTE_ID_THRESHOLD";
const EDENSCM_REMOTE_NAME_THRESHOLD: &str = "EDENSCM_REMOTE_NAME_THRESHOLD";

struct EdenApiProtocol {
    client: Arc<dyn EdenApi>,

    /// Manually disabled names defined by `EDENSCM_DISABLE_REMOTE_RESOLVE`
    /// in the form `hex1,hex2,...`.
    disabled_names: HashSet<Vertex>,

    /// Manually disabled ID resolution after `N` entries.
    /// Set by `EDENSCM_REMOTE_ID_THRESHOLD=N`.
    remote_id_threshold: Option<usize>,
    remote_id_current: AtomicUsize,

    /// Manually disabled name resolution after `N` entries.
    /// Set by `EDENSCM_REMOTE_NAME_THRESHOLD=N`.
    remote_name_threshold: Option<usize>,
    remote_name_current: AtomicUsize,
}

fn to_dag_error<E: Into<anyhow::Error>>(e: E) -> dag::Error {
    dag::errors::BackendError::Other(e.into()).into()
}

#[async_trait]
impl RemoteIdConvertProtocol for EdenApiProtocol {
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<Vertex>,
        names: Vec<Vertex>,
    ) -> dag::Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        let mut pairs = Vec::with_capacity(names.len());
        let response_vec = {
            if heads.is_empty() {
                // Not an error case. Just do not resolve anything.
                return Ok(Vec::new());
            }
            let mut hgids = Vec::with_capacity(names.len());
            for name in names {
                if self.disabled_names.contains(&name) {
                    let msg = format!(
                        "Resolving {:?} is disabled via {}",
                        name, EDENSCM_DISABLE_REMOTE_RESOLVE
                    );
                    return Err(dag::errors::BackendError::Generic(msg).into());
                }
                if let Some(threshold) = self.remote_name_threshold {
                    let current = self.remote_name_current.fetch_add(1, SeqCst);
                    if current >= threshold {
                        let msg = format!(
                            "Resolving name {:?} exceeds threshold {} set by {}",
                            name, threshold, EDENSCM_REMOTE_NAME_THRESHOLD
                        );
                        return Err(dag::errors::BackendError::Generic(msg).into());
                    }
                }
                hgids.push(Id20::from_slice(name.as_ref()).map_err(to_dag_error)?);
            }
            let heads: Vec<_> = heads
                .iter()
                .map(|v| Id20::from_slice(v.as_ref()).map_err(to_dag_error))
                .collect::<dag::Result<Vec<_>>>()?;
            self.client
                .commit_hash_to_location(heads, hgids)
                .await
                .map_err(to_dag_error)?
        };
        for response in response_vec {
            if let Some(location) = response.result.map_err(to_dag_error)? {
                let path = AncestorPath {
                    x: Vertex::copy_from(location.descendant.as_ref()),
                    n: location.distance,
                    batch_size: 1,
                };
                let name = Vertex::copy_from(response.hgid.as_ref());
                pairs.push((path, vec![name]));
            }
        }
        Ok(pairs)
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> dag::Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        if let Some(threshold) = self.remote_id_threshold {
            let current = self.remote_id_current.fetch_add(1, SeqCst);
            if current >= threshold {
                let msg = format!(
                    "Resolving id exceeds threshold {} set by {}",
                    threshold, EDENSCM_REMOTE_ID_THRESHOLD
                );
                return Err(dag::errors::BackendError::Generic(msg).into());
            }
        }
        let mut pairs = Vec::with_capacity(paths.len());
        let response_vec = {
            let mut requests = Vec::with_capacity(paths.len());
            for path in paths {
                let descendant = Id20::from_slice(path.x.as_ref()).map_err(to_dag_error)?;
                requests.push(CommitLocationToHashRequest {
                    location: Location {
                        descendant,
                        distance: path.n,
                    },
                    count: path.batch_size,
                });
            }
            self.client
                .commit_location_to_hash(requests)
                .await
                .map_err(to_dag_error)?
        };
        for response in response_vec {
            let path = AncestorPath {
                x: Vertex::copy_from(response.location.descendant.as_ref()),
                n: response.location.distance,
                batch_size: response.count,
            };
            let names = response
                .hgids
                .into_iter()
                .map(|n| Vertex::copy_from(n.as_ref()))
                .collect();
            pairs.push((path, names));
        }
        Ok(pairs)
    }
}

impl HybridCommits {
    pub fn new(
        revlog_dir: Option<&Path>,
        dag_path: &Path,
        commits_path: &Path,
        client: Arc<dyn EdenApi>,
    ) -> Result<Self> {
        let commits = HgCommits::new(dag_path, commits_path)?;
        let revlog = match revlog_dir {
            Some(revlog_dir) => Some(RevlogCommits::new(revlog_dir)?),
            None => None,
        };
        Ok(Self {
            revlog,
            commits,
            client,
            lazy_hash_desc: "not lazy".to_string(),
        })
    }

    /// Enable fetching commit hashes lazily via EdenAPI.
    pub fn enable_lazy_commit_hashes(&mut self) {
        let mut disabled_names: HashSet<Vertex> = Default::default();
        if let Ok(env) = std::env::var(EDENSCM_DISABLE_REMOTE_RESOLVE) {
            for hex in env.split(",") {
                if let Ok(name) = Vertex::from_hex(hex.as_ref()) {
                    disabled_names.insert(name);
                }
            }
        }
        let remote_id_threshold = if let Ok(env) = std::env::var(EDENSCM_REMOTE_ID_THRESHOLD) {
            env.parse::<usize>().ok()
        } else {
            None
        };
        let remote_name_threshold = if let Ok(env) = std::env::var(EDENSCM_REMOTE_NAME_THRESHOLD) {
            env.parse::<usize>().ok()
        } else {
            None
        };
        let protocol = EdenApiProtocol {
            client: self.client.clone(),
            disabled_names,
            remote_id_threshold,
            remote_id_current: Default::default(),
            remote_name_threshold,
            remote_name_current: Default::default(),
        };
        self.commits.dag.set_remote_protocol(Arc::new(protocol));
        self.lazy_hash_desc = format!("lazy, using EdenAPI");
    }

    /// Enable fetching commit hashes lazily via another "segments".
    /// directory locally. This is for testing purpose.
    pub fn enable_lazy_commit_hashes_from_local_segments(&mut self, dag_path: &Path) -> Result<()> {
        let dag = dag::Dag::open(dag_path)?;
        self.commits.dag.set_remote_protocol(Arc::new(dag));
        self.lazy_hash_desc = format!("lazy, using local segments ({})", dag_path.display());
        Ok(())
    }
}

#[async_trait::async_trait]
impl AppendCommits for HybridCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        if let Some(revlog) = self.revlog.as_mut() {
            revlog.add_commits(commits).await?;
        }
        self.commits.add_commits(commits).await?;
        Ok(())
    }

    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        if let Some(revlog) = self.revlog.as_mut() {
            revlog.flush(master_heads).await?;
        }
        self.commits.flush(master_heads).await?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        if let Some(revlog) = self.revlog.as_mut() {
            revlog.flush_commit_data().await?;
        }
        self.commits.flush_commit_data().await?;
        self.commits.dag.flush_cached_idmap().await?;
        Ok(())
    }

    async fn add_graph_nodes(&mut self, graph_nodes: &[crate::GraphNode]) -> Result<()> {
        if self.revlog.is_some() {
            return Err(crate::Error::Unsupported(
                "add_graph_nodes is not supported for revlog backend",
            ));
        }
        self.commits.add_graph_nodes(graph_nodes).await?;
        Ok(())
    }

    async fn import_clone_data(&mut self, clone_data: CloneData<Vertex>) -> Result<()> {
        if self.revlog.is_some() {
            return Err(crate::Error::Unsupported(
                "import_clone_data is not supported for revlog backend",
            ));
        }
        if self.commits.dag.all().await?.count().await? > 0 {
            return Err(crate::Error::Unsupported(
                "import_clone_data can only be used in an empty repo",
            ));
        }
        if !self.commits.dag.is_vertex_lazy() {
            return Err(crate::Error::Unsupported(
                "import_clone_data can only be used in commit graph with lazy vertexes",
            ));
        }
        self.commits.dag.import_clone_data(clone_data).await?;
        Ok(())
    }

    async fn import_pull_data(
        &mut self,
        clone_data: CloneData<Vertex>,
        heads: &VertexListWithOptions,
    ) -> Result<()> {
        if self.revlog.is_some() {
            return Err(crate::Error::Unsupported(
                "import_pull_data is not supported for revlog backend",
            ));
        }
        if !self.commits.dag.is_vertex_lazy() {
            return Err(crate::Error::Unsupported(
                "import_pull_data can only be used in commit graph with lazy vertexes",
            ));
        }
        self.commits.dag.import_pull_data(clone_data, heads).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReadCommitText for HybridCommits {
    async fn get_commit_raw_text_list(&self, vertexes: &[Vertex]) -> Result<Vec<Bytes>> {
        let vertexes: Vec<Vertex> = vertexes.to_vec();
        let stream =
            self.stream_commit_raw_text(Box::pin(stream::iter(vertexes.into_iter().map(Ok))))?;
        let commits: Vec<Bytes> = stream.map(|c| c.map(|c| c.raw_text)).try_collect().await?;
        Ok(commits)
    }
}

impl StreamCommitText for HybridCommits {
    fn stream_commit_raw_text(
        &self,
        input: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let zstore = self.commits.commit_data_store();
        let client = self.client.clone();
        let resolver = Resolver { client, zstore };
        let buffer_size = 10000;
        let retry_limit = 0;
        let stream = HybridStream::new(input, resolver, buffer_size, retry_limit);
        let stream = stream.map_ok(|(vertex, raw_text)| ParentlessHgCommit { vertex, raw_text });
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl StripCommits for HybridCommits {
    async fn strip_commits(&mut self, set: Set) -> Result<()> {
        if let Some(revlog) = self.revlog.as_mut() {
            revlog.strip_commits(set.clone()).await?;
        }
        self.commits.strip_commits(set).await?;
        Ok(())
    }
}

struct Resolver {
    client: Arc<dyn EdenApi>,
    zstore: Arc<RwLock<Zstore>>,
}

impl Drop for Resolver {
    fn drop(&mut self) {
        // Write commit data back to zstore, best effort.
        let _ = self.zstore.write().flush();
    }
}

#[async_trait]
impl HybridResolver<Vertex, Bytes, anyhow::Error> for Resolver {
    fn resolve_local(&mut self, vertex: &Vertex) -> anyhow::Result<Option<Bytes>> {
        let id = Id20::from_slice(vertex.as_ref())?;
        match self.zstore.read().get(id)? {
            Some(bytes) => Ok(Some(bytes.slice(Id20::len() * 2..))),
            None => Ok(crate::revlog::get_hard_coded_commit_text(vertex)),
        }
    }

    #[instrument(level = "debug", skip(self))]
    async fn resolve_remote(
        &self,
        input: &[Vertex],
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<(Vertex, Bytes)>>> {
        let ids: Vec<Id20> = input
            .iter()
            .map(|i| Id20::from_slice(i.as_ref()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let client = self.client.clone();
        let response = client.commit_revlog_data(ids).await?;
        let zstore = self.zstore.clone();
        let commits = response.entries.map(move |e| {
            let e = e?;
            let written_id = zstore.write().insert(&e.revlog_data, &[])?;
            if !written_id.is_null() && written_id != e.hgid {
                anyhow::bail!(
                    "server returned commit-text pair ({}, {:?}) has mismatched SHA1: {}",
                    e.hgid.to_hex(),
                    e.revlog_data,
                    written_id.to_hex(),
                );
            }
            let bytes = &e.revlog_data[Id20::len() * 2..];
            let input_output = (
                Vertex::copy_from(e.hgid.as_ref()),
                Bytes::copy_from_slice(&bytes),
            );
            Ok(input_output)
        });
        Ok(Box::pin(commits) as BoxStream<'_, _>)
    }

    fn retry_error(&self, _attempt: usize, input: &[Vertex]) -> anyhow::Error {
        anyhow::format_err!("cannot resolve {:?} remotely", input)
    }
}

delegate!(CheckIntegrity | IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, HybridCommits => self.commits);

impl DescribeBackend for HybridCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        let (backend, revlog_path, revlog_usage) = match self.revlog.as_ref() {
            Some(revlog) => {
                let path = revlog.dir.join("00changelog.{i,d,nodemap}");
                (
                    "hybrid",
                    path.display().to_string(),
                    "present, not used for reading",
                )
            }
            None => ("lazytext", "(not used)".to_string(), "(not used)"),
        };
        format!(
            r#"Backend ({}):
  Local:
    Segments + IdMap: {}
    Zstore: {}
    Revlog + Nodemap: {}
Feature Providers:
  Commit Graph Algorithms:
    Segments
  Commit Hash / Rev Lookup:
    IdMap
  Commit Data (user, message):
    Zstore (incomplete, draft)
    EdenAPI (remaining, public)
    Revlog {}
Commit Hashes: {}
"#,
            backend,
            self.commits.dag_path.display(),
            self.commits.commits_path.display(),
            revlog_path,
            revlog_usage,
            &self.lazy_hash_desc,
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        self.commits.explain_internals(w)
    }
}
