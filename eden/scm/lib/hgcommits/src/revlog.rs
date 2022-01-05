/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use dag::delegate;
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
    pub fn new(dir: &Path) -> Result<Self> {
        let index_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");
        let revlog = RevlogIndex::new(&index_path, &nodemap_path)?;
        Ok(Self {
            revlog,
            dir: dir.to_path_buf(),
        })
    }
}

#[async_trait::async_trait]
impl AppendCommits for RevlogCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        for commit in commits {
            let mut parent_revs = Vec::with_capacity(commit.parents.len());
            for parent in &commit.parents {
                parent_revs.push(self.revlog.vertex_id(parent.clone()).await?.0 as u32);
            }
            self.revlog
                .insert(commit.vertex.clone(), parent_revs, commit.raw_text.clone())
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
        let mut new = Self::new(&new_dir)?;
        strip::migrate_commits(self, &mut new, set).await?;
        drop(new);
        strip::racy_unsafe_move_files(&new_dir, old_dir)?;
        *self = Self::new(old_dir)?;
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
