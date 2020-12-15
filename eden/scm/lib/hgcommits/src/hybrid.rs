/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
use async_trait::async_trait;
use dag::delegate;
use dag::Set;
use dag::Vertex;
use edenapi::EdenApi;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use minibytes::Bytes;
use parking_lot::RwLock;
use std::io;
use std::path::Path;
use std::sync::Arc;
use streams::HybridResolver;
use streams::HybridStream;
use tracing::instrument;
use zstore::Id20;
use zstore::Zstore;

/// Segmented Changelog + Revlog + Remote.
///
/// Use segmented changelog for the commit graph algorithms and IdMap.
/// Writes to revlog just for fallback.
///
/// Use edenapi to resolve public commit messages.
pub struct HybridCommits {
    revlog: RevlogCommits,
    commits: HgCommits,
    client: Arc<dyn EdenApi>,
    reponame: String,
}

impl HybridCommits {
    pub fn new(
        revlog_dir: &Path,
        dag_path: &Path,
        commits_path: &Path,
        client: Arc<dyn EdenApi>,
        reponame: String,
    ) -> Result<Self> {
        let commits = HgCommits::new(dag_path, commits_path)?;
        let revlog = RevlogCommits::new(revlog_dir)?;
        Ok(Self {
            revlog,
            commits,
            client,
            reponame,
        })
    }
}

#[async_trait::async_trait]
impl AppendCommits for HybridCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        self.revlog.add_commits(commits).await?;
        self.commits.add_commits(commits).await?;
        Ok(())
    }

    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        self.revlog.flush(master_heads).await?;
        self.commits.flush(master_heads).await?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        self.revlog.flush_commit_data().await?;
        self.commits.flush_commit_data().await?;
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
        let reponame = self.reponame.clone();
        let resolver = Resolver {
            client,
            zstore,
            reponame,
        };
        let buffer_size = 5000;
        let retry_limit = 0;
        let stream = HybridStream::new(input, resolver, buffer_size, retry_limit);
        let stream = stream.map_ok(|(vertex, raw_text)| ParentlessHgCommit { vertex, raw_text });
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl StripCommits for HybridCommits {
    async fn strip_commits(&mut self, set: Set) -> Result<()> {
        self.revlog.strip_commits(set.clone()).await?;
        self.commits.strip_commits(set).await?;
        Ok(())
    }
}

struct Resolver {
    client: Arc<dyn EdenApi>,
    zstore: Arc<RwLock<Zstore>>,
    reponame: String,
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
        // Mercurial hard-coded special-case that does not match SHA1.
        if id.is_null() || id.is_wdir() {
            return Ok(Some(Default::default()));
        }
        match self.zstore.read().get(id)? {
            Some(bytes) => Ok(Some(bytes.slice(Id20::len() * 2..))),
            None => Ok(None),
        }
    }

    #[instrument(level = "debug", skip(self))]
    async fn resolve_remote(
        &mut self,
        input: &[Vertex],
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<(Vertex, Bytes)>>> {
        let ids: Vec<Id20> = input
            .iter()
            .map(|i| Id20::from_slice(i.as_ref()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let reponame = self.reponame.clone();
        let client = self.client.clone();
        let progress = None;
        let response = client.commit_revlog_data(reponame, ids, progress).await?;
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

delegate!(IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, HybridCommits => self.commits);

impl DescribeBackend for HybridCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (hybrid):
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
    Revlog (present, not used for reading)
"#,
            self.commits.dag_path.display(),
            self.commits.commits_path.display(),
            self.revlog.dir.join("00changelog.{i,d,nodemap}").display(),
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        self.commits.explain_internals(w)
    }
}
