/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use dag::delegate;
use dag::errors::NotFoundError;
use dag::ops::DagPersistent;
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use gitdag::git2;
use gitdag::GitDag;
use metalog::MetaLog;
use minibytes::Bytes;
use parking_lot::Mutex;
use storemodel::ReadRootTreeIds;
use types::HgId;

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
    git_repo: Arc<Mutex<git2::Repository>>,
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
            git_repo: Arc::new(Mutex::new(git_repo)),
            dag,
            dag_path,
            git_path,
        })
    }

    /// Rewrite metalog bookmarks, remotenames to match git references.
    /// The reverse of `metalog_to_git_references`, used at the start of a transaction.
    pub fn git_references_to_metalog(&self, metalog: &mut MetaLog) -> Result<()> {
        let refs = self.dag.git_references();

        let mut bookmarks = BTreeMap::new();
        let mut remotenames = BTreeMap::new();
        let mut visibleheads = Vec::new();

        for (name, vertex) in refs {
            let names: Vec<&str> = name.splitn(3, '/').collect();
            let id = match HgId::from_slice(vertex.as_ref()) {
                Ok(id) => id,
                Err(_) => continue,
            };
            match &names[..] {
                ["refs", "remotes", name] => {
                    // Treat as a remotename
                    if name.contains('/') && !name.ends_with("/HEAD") && !name.starts_with("tags/")
                    {
                        remotenames.insert(name.to_string(), id);
                    }
                }
                ["refs", "heads", name] => {
                    // Treat as a local bookmark.
                    if name != &"HEAD" {
                        bookmarks.insert(name.to_string(), id);
                    }
                }
                ["refs", "tags", name] => {
                    // Treat as a remotename prefixed with `tags/`.
                    if name != &"HEAD" {
                        let name = format!("tags/{}", name);
                        remotenames.insert(name, id);
                    }
                }
                ["refs", "visibleheads", _name] => {
                    visibleheads.push(id);
                }
                _ => {}
            }
        }

        let encoded_bookmarks = refencode::encode_bookmarks(&bookmarks);
        let encoded_remotenames = refencode::encode_remotenames(&remotenames);
        let encoded_visibleheads = refencode::encode_visibleheads(&visibleheads);
        metalog.set("bookmarks", encoded_bookmarks.as_ref())?;
        metalog.set("remotenames", encoded_remotenames.as_ref())?;
        metalog.set("visibleheads", encoded_visibleheads.as_ref())?;
        let mut opts = metalog::CommitOptions::default();
        opts.message = "sync from git";
        metalog.commit(opts)?;

        Ok(())
    }

    /// Rewrite git references to match bookmarks, remotenames in metalog.
    /// The reverse of `git_references_to_metalog`, used at the end of a transaction.
    pub fn metalog_to_git_references(&self, metalog: &MetaLog) -> Result<()> {
        let expected_refs = {
            let mut refs: BTreeMap<String, git2::Oid> = Default::default();
            if let Some(encoded) = metalog.get("bookmarks")? {
                let decoded = refencode::decode_bookmarks(&encoded)?;
                for (name, hgid) in decoded {
                    let name = format!("refs/heads/{}", name);
                    refs.insert(name, hgid_to_git_oid(hgid));
                }
            }
            if let Some(encoded) = metalog.get("remotenames")? {
                let decoded = refencode::decode_remotenames(&encoded)?;
                for (name, hgid) in decoded {
                    let name = if let Some(tag) = name.strip_prefix("tags/") {
                        format!("refs/tags/{}", tag)
                    } else {
                        format!("refs/remotes/{}", name)
                    };
                    refs.insert(name, hgid_to_git_oid(hgid));
                }
            }
            if let Some(encoded) = metalog.get("visibleheads")? {
                let decoded = refencode::decode_visibleheads(&encoded)?;
                for hgid in decoded {
                    let name = format!("refs/visibleheads/{}", hgid.to_hex());
                    refs.insert(name, hgid_to_git_oid(hgid));
                }
            }
            refs
        };

        {
            let reflog_message = format!(
                "{}\nRootId: {}",
                metalog.message(),
                metalog.root_id().to_hex()
            );
            let repo = self.git_repo.lock();
            let mut handled_ref_names = HashSet::with_capacity(expected_refs.len());
            for reference in repo.references()? {
                let mut reference = reference?;
                let name = match reference.name() {
                    Some(n) => n,
                    None => continue,
                };
                handled_ref_names.insert(name.to_string());
                // Only care about managed ref names. Skip HEAD or FETCH_HEAD
                // or refs/something_else/*. See git_references_to_metalog
                // for managed refs.
                let names: Vec<&str> = name.splitn(3, '/').collect();
                let managed: bool = match &names[..] {
                    ["refs", "remotes", _] => true,
                    ["refs", "heads", _] => true,
                    ["refs", "tags", _] => true,
                    ["refs", "visibleheads", _] => true,
                    _ => false,
                };
                if !managed {
                    continue;
                }
                let expected_oid = expected_refs.get(name);
                match expected_oid {
                    None => reference.delete()?,
                    Some(&oid) => {
                        if let Ok(obj) = reference.peel(git2::ObjectType::Commit) {
                            if obj.id() != oid {
                                repo.reference(name, oid, true, &reflog_message)?;
                            }
                        }
                    }
                }
            }
            for (name, oid) in expected_refs {
                if handled_ref_names.contains(name.as_str()) {
                    continue;
                }
                repo.reference(&name, oid, true, &reflog_message)?;
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl AppendCommits for GitSegmentedCommits {
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        // Write to git odb.
        // Raw text format should be in git, although the type name is HgCommit.
        {
            let repo = self.git_repo.lock();
            let odb = repo.odb()?;
            for commit in commits {
                let oid = odb.write(git2::ObjectType::Commit, commit.raw_text.as_ref())?;
                if oid.as_ref() != commit.vertex.as_ref() {
                    return Err(crate::Error::HashMismatch(
                        Vertex::copy_from(oid.as_ref()),
                        commit.vertex.clone(),
                    ));
                }
            }
        }

        // Write to segments.
        let graph_nodes = utils::commits_to_graph_nodes(commits);
        self.add_graph_nodes(&graph_nodes).await?;

        Ok(())
    }

    async fn add_graph_nodes(&mut self, graph_nodes: &[GraphNode]) -> Result<()> {
        utils::add_graph_nodes_to_dag(&mut *self.dag, graph_nodes).await
    }

    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        let heads = VertexListWithOptions::from(master_heads).with_highest_group(Group::MASTER);
        self.dag.flush(&heads).await?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        Ok(())
    }

    fn update_references_to_match_metalog(&mut self, metalog: &MetaLog) -> Result<()> {
        self.metalog_to_git_references(metalog)
    }
}

#[async_trait::async_trait]
impl ReadCommitText for GitSegmentedCommits {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        self.git_repo.get_commit_raw_text(vertex).await
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        self.git_repo.to_dyn_read_commit_text()
    }

    fn to_dyn_read_root_tree_ids(&self) -> Arc<dyn ReadRootTreeIds + Send + Sync> {
        // The default impl works. But ReadCommitText has overhead constructing
        // the hg text. Bypass that overhead.
        Arc::new(Wrapper(self.git_repo.clone()))
    }
}

#[async_trait::async_trait]
impl ReadCommitText for Arc<Mutex<git2::Repository>> {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let oid = match git2::Oid::from_bytes(vertex.as_ref()) {
            Ok(oid) => oid,
            Err(_) => return Ok(None),
        };
        let repo = self.lock();
        let commit = match repo.find_commit(oid) {
            Ok(commit) => commit,
            Err(e) if e.code() == git2::ErrorCode::NotFound => {
                return Ok(crate::revlog::get_hard_coded_commit_text(vertex));
            }
            Err(e) => return Err(e.into()),
        };
        let text = to_hg_text(&commit);
        Ok(Some(text))
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }
}

// Workaround orphan rule
struct Wrapper<T>(T);

#[async_trait::async_trait]
impl ReadRootTreeIds for Wrapper<Arc<Mutex<git2::Repository>>> {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let mut result = Vec::with_capacity(commits.len());
        let repo = self.0.lock();
        for commit_hgid in commits {
            let oid = hgid_to_git_oid(commit_hgid);
            let commit = repo.find_commit(oid)?;
            let tree_id = commit.tree_id();
            let tree_hgid =
                HgId::from_slice(tree_id.as_bytes()).expect("git Oid should convert to HgId");
            result.push((commit_hgid, tree_hgid));
        }
        Ok(result)
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
    // 222 is calculated from debugshell in linux.git:
    // max(len(cl.revision(n))-len(repo[n].description().encode('utf8')) for n in cl.dag.all().take(50000))
    let len = commit.message_bytes().len() + 222;
    let mut result = Vec::with_capacity(len);
    let mut write = |s: &[u8]| result.extend_from_slice(s);

    fn utf8<'a>(s: &'a [u8]) -> Cow<'a, str> {
        String::from_utf8_lossy(s)
    }

    // Construct the commit using (faked) hg format:
    // manifest hex + "\n" + user + "\n" + date + (extra) + "\n" + (files) + "\n" + desc

    // manifest hex
    write(to_hex(commit.tree_id()).as_bytes());
    write(b"\n");

    // user
    let author = commit.author();
    write(utf8(author.name_bytes()).as_bytes());
    write(b" <");
    write(utf8(author.email_bytes()).as_bytes());
    write(b">\n");

    // date
    write(to_hg_date_text(&author.when()).as_bytes());

    // extras (committer)
    let committer = commit.committer();
    write(b" committer:");
    write(utf8(committer.name_bytes()).as_bytes());
    write(b" <");
    write(utf8(committer.email_bytes()).as_bytes());
    write(b">\0committer_date:");
    write(to_hg_date_text(&committer.when()).as_bytes());
    write(b"\n");

    // files
    // NOTE: currently ignored.
    write(b"\n");

    // message
    write(utf8(commit.message_bytes()).as_bytes());

    result.into()
}

fn hgid_to_git_oid(id: HgId) -> git2::Oid {
    git2::Oid::from_bytes(id.as_ref()).expect("HgId should convert to git2::Oid")
}
