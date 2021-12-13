/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::path::Path;
use std::path::PathBuf;

use dag::delegate;
use dag::errors::NotFoundError;
use dag::Set;
use dag::Vertex;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use gitdag::git2;
use gitdag::GitDag;
use metalog::MetaLog;
use minibytes::Bytes;
use parking_lot::Mutex;

use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StreamCommitText;
use crate::StripCommits;

/// Git Commits with segments index.
///
/// Use segmented changelog for the commit graph algorithms and IdMap.
/// Use libgit2 for commit messages.
///
/// This is currently read-only, because the `add_commits` API is
/// coupled with the HG SHA1 implementation details and Git does not
/// use HG's SHA1.
///
/// It does not support migrating to other formats, because of SHA1
/// incompatibility.
///
/// In the future when we abstract away the HG SHA1 logic, we can
/// revisit and build something writable based on this.
pub struct GitSegmentedCommits {
    git_repo: Mutex<git2::Repository>,
    dag: GitDag,
    dag_path: PathBuf,
    git_path: PathBuf,
}

impl GitSegmentedCommits {
    pub fn new(git_dir: &Path, dag_dir: &Path) -> Result<Self> {
        let git_repo = git2::Repository::open(git_dir)?;
        // open_git_repo has side effect building up the segments
        let dag = GitDag::open_git_repo(&git_repo, dag_dir, "refs/remotes/origin/master")?;
        let dag_path = dag_dir.to_path_buf();
        let git_path = git_dir.to_path_buf();
        Ok(Self {
            git_repo: Mutex::new(git_repo),
            dag,
            dag_path,
            git_path,
        })
    }

    /// Migrate git references to metalog.
    pub fn export_git_references(&self, metalog: &mut MetaLog) -> Result<()> {
        let refs = self.dag.git_references();

        let mut bookmarks = Vec::new();
        let mut remotenames = Vec::new();

        for (name, vertex) in refs {
            let names: Vec<&str> = name.splitn(3, '/').collect();
            match &names[..] {
                ["refs", "remotes", name] => {
                    // Treat as a remotename
                    if name.contains('/') && !name.ends_with("/HEAD") {
                        remotenames.push(format!("{} bookmarks {}\n", vertex.to_hex(), name));
                    }
                }
                ["refs", "tags", name] | ["refs", "heads", name] => {
                    // Treat as a bookmark
                    if name != &"HEAD" {
                        bookmarks.push(format!("{} {}\n", vertex.to_hex(), name));
                    }
                }
                _ => {}
            }
        }

        metalog.set("bookmarks", bookmarks.concat().as_bytes())?;
        metalog.set("remotenames", remotenames.concat().as_bytes())?;
        let mut opts = metalog::CommitOptions::default();
        opts.message = "sync from git";
        metalog.commit(opts)?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl AppendCommits for GitSegmentedCommits {
    async fn add_commits(&mut self, _commits: &[HgCommit]) -> Result<()> {
        Err(crate::Error::Unsupported("add commits for git backend"))
    }

    async fn flush(&mut self, _master_heads: &[Vertex]) -> Result<()> {
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReadCommitText for GitSegmentedCommits {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let oid = match git2::Oid::from_bytes(vertex.as_ref()) {
            Ok(oid) => oid,
            Err(_) => return Ok(None),
        };
        let repo = self.git_repo.lock();
        let commit = repo.find_commit(oid)?;
        let text = to_hg_text(&commit);
        Ok(Some(text))
    }
}

impl StreamCommitText for GitSegmentedCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let git_repo = git2::Repository::open(&self.git_path)?;
        let stream = stream.map(move |item| {
            let vertex = item?;
            let oid = match git2::Oid::from_bytes(vertex.as_ref()) {
                Ok(oid) => oid,
                Err(_) => return vertex.not_found().map_err(Into::into),
            };
            let commit = git_repo.find_commit(oid)?;
            let raw_text = to_hg_text(&commit);
            Ok(ParentlessHgCommit { vertex, raw_text })
        });
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl StripCommits for GitSegmentedCommits {
    async fn strip_commits(&mut self, _set: Set) -> Result<()> {
        Err(crate::Error::Unsupported("strip for git backend"))
    }
}

delegate!(CheckIntegrity | IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, GitSegmentedCommits => self.dag);

impl DescribeBackend for GitSegmentedCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (segmented git):
  Local:
    Segments + IdMap: {}
    Git: {}
Feature Providers:
  Commit Graph Algorithms:
    Segments
  Commit Hash / Rev Lookup:
    IdMap
  Commit Data (user, message):
    Git
"#,
            self.dag_path.display(),
            self.git_path.display(),
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        write!(w, "{:?}", &*self.dag)
    }
}

fn to_hex(oid: git2::Oid) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let bytes = oid.as_bytes();
    let mut v = Vec::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        v.push(HEX_CHARS[(byte >> 4) as usize]);
        v.push(HEX_CHARS[(byte & 0xf) as usize]);
    }
    unsafe { String::from_utf8_unchecked(v) }
}

fn to_hg_date_text(time: &git2::Time) -> String {
    format!("{} {}", time.seconds(), time.offset_minutes())
}

/// Convert a git commit to hg commit text.
fn to_hg_text(commit: &git2::Commit) -> Bytes {
    let mut result = Vec::new();
    let mut write = |s: &[u8]| result.extend_from_slice(s);

    // Construct the commit using (faked) hg format:
    // manifest hex + "\n" + user + "\n" + date + (extra) + "\n" + (files) + "\n" + desc

    // manifest hex
    write(to_hex(commit.tree_id()).as_bytes());
    write(b"\n");

    // user
    let author = commit.author();
    write(author.name_bytes());
    write(b" <");
    write(author.email_bytes());
    write(b">\n");

    // date
    write(to_hg_date_text(&author.when()).as_bytes());

    // extras (committer)
    let committer = commit.committer();
    write(b" committer:");
    write(committer.name_bytes());
    write(b" <");
    write(committer.email_bytes());
    write(b">\0committer_date:");
    write(to_hg_date_text(&committer.when()).as_bytes());
    write(b"\n");

    // files
    // NOTE: currently ignored.
    write(b"\n");

    // message
    write(commit.message_bytes());

    result.into()
}
