/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StripCommits;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::Vertex;
use gitdag::git2;
use gitdag::GitDag;
use metalog::MetaLog;
use minibytes::Bytes;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

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
    git_repo: git2::Repository,
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
            git_repo,
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
                _ => (),
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

impl AppendCommits for GitSegmentedCommits {
    fn add_commits(&mut self, _commits: &[HgCommit]) -> Result<()> {
        Err(crate::Error::Unsupported("add commits for git backend"))
    }

    fn flush(&mut self, _master_heads: &[Vertex]) -> Result<()> {
        Ok(())
    }
}

impl ReadCommitText for GitSegmentedCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let oid = match git2::Oid::from_bytes(vertex.as_ref()) {
            Ok(oid) => oid,
            Err(_) => return Ok(None),
        };
        let commit = self.git_repo.find_commit(oid)?;
        let text = to_hg_text(&commit);
        Ok(Some(text))
    }
}

impl StripCommits for GitSegmentedCommits {
    fn strip_commits(&mut self, _set: Set) -> Result<()> {
        Err(crate::Error::Unsupported("strip for git backend"))
    }
}

impl IdConvert for GitSegmentedCommits {
    fn vertex_id(&self, name: Vertex) -> dag::Result<Id> {
        self.dag.vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &Vertex, max_group: Group) -> dag::Result<Option<Id>> {
        self.dag.vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> dag::Result<Vertex> {
        self.dag.vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &Vertex) -> dag::Result<bool> {
        self.dag.contains_vertex_name(name)
    }
}

impl PrefixLookup for GitSegmentedCommits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> dag::Result<Vec<Vertex>> {
        self.dag.vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl DagAlgorithm for GitSegmentedCommits {
    fn sort(&self, set: &Set) -> dag::Result<Set> {
        self.dag.sort(set)
    }
    fn parent_names(&self, name: Vertex) -> dag::Result<Vec<Vertex>> {
        self.dag.parent_names(name)
    }
    fn all(&self) -> dag::Result<Set> {
        self.dag.all()
    }
    fn ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.ancestors(set)
    }
    fn parents(&self, set: Set) -> dag::Result<Set> {
        self.dag.parents(set)
    }
    fn first_ancestor_nth(&self, name: Vertex, n: u64) -> dag::Result<Vertex> {
        self.dag.first_ancestor_nth(name, n)
    }
    fn heads(&self, set: Set) -> dag::Result<Set> {
        self.dag.heads(set)
    }
    fn children(&self, set: Set) -> dag::Result<Set> {
        self.dag.children(set)
    }
    fn roots(&self, set: Set) -> dag::Result<Set> {
        self.dag.roots(set)
    }
    fn gca_one(&self, set: Set) -> dag::Result<Option<Vertex>> {
        self.dag.gca_one(set)
    }
    fn gca_all(&self, set: Set) -> dag::Result<Set> {
        self.dag.gca_all(set)
    }
    fn common_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.common_ancestors(set)
    }
    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> dag::Result<bool> {
        self.dag.is_ancestor(ancestor, descendant)
    }
    fn heads_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.heads_ancestors(set)
    }
    fn range(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.dag.range(roots, heads)
    }
    fn only(&self, reachable: Set, unreachable: Set) -> dag::Result<Set> {
        self.dag.only(reachable, unreachable)
    }
    fn only_both(&self, reachable: Set, unreachable: Set) -> dag::Result<(Set, Set)> {
        self.dag.only_both(reachable, unreachable)
    }
    fn descendants(&self, set: Set) -> dag::Result<Set> {
        self.dag.descendants(set)
    }
    fn reachable_roots(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.dag.reachable_roots(roots, heads)
    }
    fn snapshot_dag(&self) -> dag::Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        self.dag.snapshot_dag()
    }
}

impl ToIdSet for GitSegmentedCommits {
    fn to_id_set(&self, set: &Set) -> dag::Result<IdSet> {
        self.dag.to_id_set(set)
    }
}

impl ToSet for GitSegmentedCommits {
    fn to_set(&self, set: &IdSet) -> dag::Result<Set> {
        self.dag.to_set(set)
    }
}

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
