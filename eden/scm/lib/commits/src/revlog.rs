/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::ensure;
use dag::delegate;
use dag::errors::programming;
use dag::errors::NotFoundError;
use dag::nonblocking::non_blocking_result;
use dag::ops::IdConvert;
use dag::Group;
use dag::Set;
use dag::Vertex;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use minibytes::Bytes;
use revlogindex::RevlogIndex;
use storemodel::SerializationFormat;
use zstore::Id20;

use crate::strip;
use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StreamCommitText;
use crate::StripCommits;

/// HG commits stored on disk using the revlog format.
#[derive(Clone)]
pub struct RevlogCommits {
    revlog: RevlogIndex,
    pub(crate) dir: PathBuf,
    format: SerializationFormat,
}

/// Hardcoded commit hashes defied by hg.
pub(crate) fn get_hard_coded_commit_text(vertex: &Vertex) -> Option<Bytes> {
    let vertex = vertex.as_ref();
    if vertex == Id20::null_id().as_ref() || vertex == Id20::wdir_id().as_ref() {
        Some(Default::default())
    } else {
        None
    }
}

impl RevlogCommits {
    pub fn new(dir: &Path, format: SerializationFormat) -> Result<Self> {
        ensure!(
            matches!(format, SerializationFormat::Hg),
            "RevlogCommits does not support Git format"
        );
        let index_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");
        let revlog = RevlogIndex::new(&index_path, &nodemap_path)?;
        Ok(Self {
            revlog,
            dir: dir.to_path_buf(),
            format,
        })
    }
}

#[async_trait::async_trait]
impl AppendCommits for RevlogCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        // Topo sort nodes since SaplingRemoteAPI returns nodes sorted lexically.
        // We try to keep a stable order relative to the input since revlog
        // insertion order can affect tests.

        let mut vertex_to_commit: HashMap<Vertex, &HgCommit> =
            commits.iter().map(|c| (c.vertex.clone(), c)).collect();
        // Tracks reverse dependency so we can enqueue the child after parent is added.
        let mut parent_to_children: HashMap<Vertex, Vec<&HgCommit>> = HashMap::new();
        // Counter to know when all a child's parents have been added.
        let mut parent_count: HashMap<Vertex, usize> = HashMap::new();
        // Queue of nodes not waiting on parents to be processed.
        let mut queue: Vec<&HgCommit> = Vec::new();

        for c in commits {
            let mut pending_parents = 0;
            for pv in c.parents.iter() {
                if let Some(pc) = vertex_to_commit.get(pv) {
                    parent_to_children
                        .entry(pc.vertex.clone())
                        .or_default()
                        .push(c);
                    pending_parents += 1;
                }
            }
            if pending_parents == 0 {
                // Parents are not present in args - assume we are good to go.
                queue.push(c);
            } else {
                parent_count.insert(c.vertex.clone(), pending_parents);
            }
        }

        while let Some(commit) = queue.pop() {
            let mut parent_revs = Vec::with_capacity(commit.parents.len());
            for parent in &commit.parents {
                parent_revs.push(self.revlog.vertex_id(parent.clone()).await?.0 as u32);
            }
            self.revlog
                .insert(commit.vertex.clone(), parent_revs, commit.raw_text.clone());

            // Remove so we can make sure we processed all the nodes, later.
            vertex_to_commit.remove(&commit.vertex);

            for child in parent_to_children
                .get(&commit.vertex)
                .map(|v| v.as_slice())
                .unwrap_or_default()
            {
                if let Some(parent_count) = parent_count.get_mut(&child.vertex) {
                    *parent_count -= 1;
                    if *parent_count == 0 {
                        // We were this child's last pending parent.
                        queue.push(child);
                    }
                }
            }
        }

        if !vertex_to_commit.is_empty() {
            programming("commits form a cycle when adding to revlog")?;
        }

        Ok(())
    }

    async fn flush(&mut self, _master_heads: &[Vertex]) -> Result<()> {
        self.revlog.flush()?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        self.revlog.flush()?;
        Ok(())
    }

    async fn update_virtual_nodes(&mut self, _wdir_parents: Vec<Vertex>) -> Result<()> {
        // XXX: Dummy implementation - revlog is rarely used.
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReadCommitText for RevlogCommits {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        match self
            .vertex_id_with_max_group(vertex, Group::NON_MASTER)
            .await?
        {
            Some(id) => Ok(Some(self.revlog.raw_data(id.0 as u32)?)),
            None => Ok(get_hard_coded_commit_text(vertex)),
        }
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }
}

impl StreamCommitText for RevlogCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let revlog = self.revlog.get_snapshot();
        let stream = stream.map(move |item| {
            let vertex = item?;
            // Mercurial hard-coded special-case that does not match SHA1.
            if let Some(raw_text) = get_hard_coded_commit_text(&vertex) {
                return Ok(ParentlessHgCommit { vertex, raw_text });
            }
            match non_blocking_result(revlog.vertex_id_with_max_group(&vertex, Group::NON_MASTER))?
            {
                Some(id) => {
                    let raw_text = revlog.raw_data(id.0 as u32)?;
                    Ok(ParentlessHgCommit { vertex, raw_text })
                }
                None => vertex.not_found().map_err(Into::into),
            }
        });
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl StripCommits for RevlogCommits {
    async fn strip_commits(&mut self, set: Set) -> Result<()> {
        let old_dir = &self.dir;
        let new_dir = old_dir.join("strip");
        let _ = fs::create_dir(&new_dir);
        let mut new = Self::new(&new_dir, SerializationFormat::Hg)?;
        strip::migrate_commits(self, &mut new, set).await?;
        drop(new);
        strip::racy_unsafe_move_files(&new_dir, old_dir)?;
        *self = Self::new(old_dir, SerializationFormat::Hg)?;
        Ok(())
    }
}

delegate!(CheckIntegrity | IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, RevlogCommits => self.revlog);

impl DescribeBackend for RevlogCommits {
    fn algorithm_backend(&self) -> &'static str {
        "revlog"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (revlog):
  Local:
    Revlog: {}
    Nodemap: {}
Feature Providers:
  Commit Graph Algorithms:
    Revlog
  Commit Hash / Rev Lookup:
    Nodemap
  Commit Data (user, message):
    Revlog
"#,
            self.dir.join("00changelog.{i,d}").display(),
            self.dir.join("00changelog.nodemap").display(),
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        writeln!(w, "(RevlogIndex explain_internals is not yet implemented)")
    }
}
