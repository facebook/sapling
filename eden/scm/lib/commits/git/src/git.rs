/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use commits_trait::AppendCommits;
use commits_trait::DagCommits;
use commits_trait::DescribeBackend;
use commits_trait::GraphNode;
use commits_trait::HgCommit;
use commits_trait::ParentlessHgCommit;
use commits_trait::ReadCommitText;
use commits_trait::StreamCommitText;
use commits_trait::StripCommits;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::Text;
use dag::delegate;
use dag::errors::NotFoundError;
use dag::ops::DagPersistent;
use dag::ops::IdConvert;
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::VertexOptions;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use gitcompat::BareGit;
use gitcompat::ReferenceValue;
use gitdag::git2;
use gitdag::GitDag;
use metalog::MetaLog;
use minibytes::Bytes;
use parking_lot::Mutex;
use paste::paste;
use storemodel::ReadRootTreeIds;
use types::HgId;

use crate::ref_filter::GitRefFilter;
use crate::ref_matcher::GitRefMatcher;
use crate::utils;

/// Git commits with segments index.
///
/// Use segmented changelog for the commit graph algorithms and IdMap.
/// Use libgit2 for commit messages.
///
/// ## Integration with metalog
///
/// Coupled with metalog to support bi-directional sync from and to Git.
/// Supports 2 sync modes:
/// - "Full" sync for "managed git". In this mode, the repo is created
///   by `sl` and selective pull is enabled. There is no
///   `fetch = +refs/heads/*:refs/remotes/origin/*` in the repo config.
///   We do not use the default `git clone` or `git fetch` to fetch all
///   references but always `fetch X:Y` to fetch "selected" references.
///   Both Git referencesa and Sapling metalog have O(1) entries.
///   Selecting more remote refs (to pull/push) requires explicit
///   commands.
///   In this mode, we can simply sync all git references without much
///   filtering.
/// - "Partial" sync for "transparent git". In this mode, the repo is
///   created by `git` and all remote branches and tags are synced to
///   local by default. We still want the "selective pull" behavior.
///   So unlike the above mode, we will apply extra filtering
///   (GitRefFilter) to only select a (hopefully O(1)) subset of Git
///   refs to sync.
///
/// About tags:
/// - Tags are treated as remote bookmarks, like "origin/tags/v1".
/// - There is no local tags. Local Git tags are simply ignored.
///
/// ## Integration with autopull
///
/// Autopull might run `git fetch ... ...:refs/visibleheads/...`.
/// Read the `refs/visibleheads` namespace to pick up changes.
///
/// ## No read write access
///
/// This is currently read-only, because the `add_commits` API is
/// coupled with the HG SHA1 implementation details and Git does not
/// use HG's SHA1. It would be nice to abstract the `add_commits`
/// API so it does not require using the HG format.
///
/// To create commits, create the underlying Git objects directly,
/// then sync the Git references.
///
/// ## No format migration
///
/// It does not support migrating to other formats via
/// `debugchangelog --migrate`, because of SHA1 incompatibility.
pub struct GitSegmentedCommits {
    git_repo: Arc<Mutex<git2::Repository>>,
    dag: GitDag,
    dag_path: PathBuf,
    git: BareGit,
    is_dotgit: bool,
    // Read from config
    config_hoist: Option<Text>,
    config_selective_pull_default: HashSet<String>,
}

impl DagCommits for GitSegmentedCommits {}

impl GitSegmentedCommits {
    pub fn new(
        git_dir: &Path,
        dag_dir: &Path,
        config: &dyn Config,
        is_dotgit: bool,
    ) -> Result<Self> {
        let git_repo = git2::Repository::open(git_dir)?;
        let dag = GitDag::open(dag_dir, git_dir)?;
        let dag_path = dag_dir.to_path_buf();
        let git_path = git_dir.to_path_buf();
        let git = BareGit::from_git_dir_and_config(git_path, config);
        let config_hoist = config.get("remotenames", "hoist");
        let config_selective_pull_default =
            config.get_or_default("remotenames", "selectivepulldefault")?;
        Ok(Self {
            git_repo: Arc::new(Mutex::new(git_repo)),
            dag,
            dag_path,
            is_dotgit,
            git,
            config_hoist,
            config_selective_pull_default,
        })
    }

    /// Rewrite metalog bookmarks, remotenames to match git references.
    /// Import related commits to segments.
    /// This is the reverse of `metalog_to_git_references`.
    /// Intended to be used at "open" time and at the start of a transaction.
    ///
    /// If `self.is_dotgit` is true, then the sync rules are a bit different:
    /// - Bookmarks: if name matches a remote branch, or "main", "master", they
    ///   will be skipped.
    /// - Remote names: if they don't already exist in metalog, and don't match
    ///   the remote "HEAD", they will be skipped.
    pub fn import_from_git(&mut self, metalog: &mut MetaLog) -> Result<()> {
        tracing::info!("updating metalog from git refs");

        let matcher = GitRefMatcher::new();

        let refs: BTreeMap<String, ReferenceValue> = self.git.list_references(Some(&matcher))?;

        // Bookmarks and remotenames are built from scratch.
        let mut bookmarks = BTreeMap::new();
        let mut remotenames = BTreeMap::new();
        let mut visibleheads = Vec::new();

        let existing_visibleheads: HashSet<_> = metalog.get_visibleheads()?.into_iter().collect();
        let existing_remotenames = metalog.get_remotenames()?;
        let existing_bookmarks = metalog.get_bookmarks()?;

        // Heads (vertexes) to import to dag.
        let mut heads = Vec::new();
        let head_opts = VertexOptions::default();

        let remote_name_filter: GitRefFilter = if self.is_dotgit {
            GitRefFilter::new_for_dotgit(
                &refs,
                &existing_remotenames,
                self.config_hoist.as_deref(),
                &self.config_selective_pull_default,
            )?
        } else {
            GitRefFilter::new_for_dotsl(&refs)?
        };

        for (name, value) in &refs {
            // Ignore symlink refs (usually just "*/HEAD"). They are handled elsewhere.
            let id = match value {
                ReferenceValue::Sym(_) => continue,
                ReferenceValue::Id(id) => *id,
            };
            let names: Vec<&str> = name.splitn(3, '/').collect();
            match &names[..] {
                ["refs", "remotes", name] => {
                    if remote_name_filter.should_import_remote_name(name) {
                        let should_import_to_dag = match existing_remotenames.get(*name) {
                            Some(&existing_id) => existing_id != id,
                            None => true,
                        };
                        if should_import_to_dag {
                            let mut opts = head_opts.clone();
                            if remote_name_filter.is_main_remote_name(name) {
                                opts.desired_group = Group::MASTER;
                            }
                            heads.push((Vertex::copy_from(id.as_ref()), opts));
                        }
                        remotenames.insert(name.to_string(), id);
                    }
                }
                ["refs", "remotetags", name] => {
                    // origin/v1 (name) => origin/tags/v1 (remotename in metalog)
                    let remotename = match name.split_once('/') {
                        Some((remote, name)) => format!("{}/tags/{}", remote, name),
                        None => continue,
                    };
                    let should_import_to_dag = match existing_remotenames.get(&remotename) {
                        Some(&existing_id) => existing_id != id,
                        None => true,
                    };
                    if should_import_to_dag {
                        heads.push((Vertex::copy_from(id.as_ref()), head_opts.clone()));
                    }
                    remotenames.insert(remotename, id);
                }
                ["refs", "heads", name] => {
                    let should_import_to_dag = match existing_bookmarks.get(*name) {
                        Some(&existing_id) => existing_id == id,
                        None => !existing_visibleheads.contains(&id),
                    };
                    if should_import_to_dag {
                        heads.push((Vertex::copy_from(id.as_ref()), head_opts.clone()));
                    }
                    if remote_name_filter.should_treat_local_ref_as_visible_head(name) {
                        visibleheads.push(id);
                    } else {
                        bookmarks.insert(name.to_string(), id);
                    }
                }
                ["refs", "visibleheads", _name] => {
                    if !existing_visibleheads.contains(&id) {
                        heads.push((Vertex::copy_from(id.as_ref()), head_opts.clone()));
                    }
                    visibleheads.push(id);
                }
                ["HEAD"] => {
                    heads.push((Vertex::copy_from(id.as_ref()), head_opts.clone()));
                }
                _ => {}
            }
        }

        let heads = VertexListWithOptions::from(heads).sort_by_group();

        tracing::trace!(
            ?remotenames,
            ?bookmarks,
            ?visibleheads,
            ?existing_remotenames,
            ?existing_bookmarks,
            ?existing_visibleheads,
            ?heads,
            "git import"
        );

        let git_repo = self.git_repo.lock();
        self.dag.import_from_git(Some(&*git_repo), heads)?;

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

    /// Import specific Git refs to metalog and the dag.
    ///
    /// Unlike `import_from_git` this is incremental,
    /// nothing outside the specified refs will be changed.
    fn import_specified_refs_from_git(
        &mut self,
        metalog: &mut MetaLog,
        ref_names: &[String],
    ) -> Result<()> {
        tracing::info!(?ref_names, "import git refs");

        #[derive(Default)]
        struct State {
            // state (lazy loaded)
            bookmarks: Option<BTreeMap<String, HgId>>,
            remotenames: Option<BTreeMap<String, HgId>>,
            visibleheads: Option<Vec<HgId>>,
            // heads to import to dag
            heads: Vec<Vertex>,
        }

        macro_rules! load {
            ($self:ident, $field:ident, $metalog:ident) => {
                paste! {
                    if let Some(ref mut v) = $self.$field {
                        v
                    } else {
                        $self.$field = Some($metalog.[<get_$field>]()?);
                        $self.$field.as_mut().unwrap()
                    }
                }
            };
        }

        macro_rules! update_map {
            ($field:ident) => {
                paste! {
                    fn [<update_ $field>](&mut self, metalog: &MetaLog, name: String, value: Option<HgId>) -> Result<()> {
                        let map = load!(self, $field, metalog);
                        match value {
                            None => { map.remove(&name); }
                            Some(id) => {
                                let orig_id = map.insert(name, id);
                                if orig_id != Some(id) { self.mark_add_head(id); }
                            }
                        }
                        Ok(())
                    }
                }
            };
        }

        impl State {
            update_map!(bookmarks);
            update_map!(remotenames);

            fn insert_visiblehead(&mut self, metalog: &MetaLog, id: HgId) -> Result<()> {
                let list = load!(self, visibleheads, metalog);
                if list.contains(&id) {
                    return Ok(());
                }
                list.push(id);
                self.mark_add_head(id);
                Ok(())
            }

            fn mark_add_head(&mut self, id: HgId) {
                self.heads.push(Vertex::copy_from(id.as_ref()));
            }
        }

        let mut state = State::default();
        for ref_name in ref_names {
            let ref_value = self.git.lookup_reference_follow_links(ref_name)?;
            if let Some(name) = ref_name.strip_prefix("refs/heads/") {
                state.update_bookmarks(metalog, name.into(), ref_value)?;
            } else if let Some(name) = ref_name.strip_prefix("refs/remotes/") {
                state.update_remotenames(metalog, name.into(), ref_value)?;
            } else if let Some(rest) = ref_name.strip_prefix("refs/remotetags/") {
                let name = match rest.split_once('/') {
                    Some((remote, name)) => format!("{}/tags/{}", remote, name),
                    None => bail!("illformed ref_name: {}", ref_name),
                };
                state.update_remotenames(metalog, name, ref_value)?;
            } else if ref_name.starts_with("refs/visibleheads/") {
                if let Some(id) = ref_value {
                    state.insert_visiblehead(metalog, id)?;
                }
            } else if let Some(id) = ref_value {
                state.mark_add_head(id);
            };
        }

        let State {
            bookmarks,
            remotenames,
            visibleheads,
            heads,
        } = state;

        tracing::trace!(
            ?heads,
            ?bookmarks,
            ?remotenames,
            ?visibleheads,
            "calculated import git refs"
        );

        if !heads.is_empty() {
            let git_repo = self.git_repo.lock();
            self.dag.import_from_git(Some(&*git_repo), heads.into())?;
        }

        if let Some(v) = bookmarks {
            metalog.set_bookmarks(&v)?;
        }
        if let Some(v) = remotenames {
            metalog.set_remotenames(&v)?;
        }
        if let Some(v) = visibleheads {
            metalog.set_visibleheads(&v)?;
        }

        let mut opts = metalog::CommitOptions::default();
        let message = format!("sync from git refs: {:?}", ref_names);
        opts.message = &message;
        metalog.commit(opts)?;

        Ok(())
    }

    /// Update git references to match metalog changes.
    /// - remotenames, bookmarks: changes will be applied to Git references.
    /// - visibleheads: current state will replace refs/visibleheads/ namespace.
    ///
    /// The reverse of `git_references_to_metalog`, used at the end of a transaction.
    fn export_to_git(&self, metalog: &MetaLog) -> Result<()> {
        tracing::info!("updating git refs from metalog");
        if tracing::enabled!(tracing::Level::TRACE) {
            let remotenames = metalog.get_remotenames()?;
            let bookmarks = metalog.get_bookmarks()?;
            let visibleheads = metalog.get_visibleheads()?;
            tracing::trace!(remotenames=?remotenames, bookmarks=?bookmarks, visibleheads=?visibleheads, "metalog (to sync to git)");
        }
        let reflog_message = format!(
            "Sync from Sapling: {}\nRootId: {}",
            metalog.message(),
            metalog.root_id().to_hex()
        );
        let repo = self.git_repo.lock();
        let mut ref_to_change = HashMap::<String, Option<git2::Oid>>::new();

        // Update visibleheads in refs/visibleheads/.
        {
            let visibleheads = metalog.get_visibleheads()?;
            let visibleheads: HashSet<HgId> = visibleheads.into_iter().collect();
            let mut git_visibleheads = HashSet::with_capacity(visibleheads.len());
            // Delete non-existed visibleheads.
            for reference in repo.references()? {
                let mut reference = reference?;
                let ref_name = match reference.name() {
                    Some(n) => n,
                    None => continue,
                };
                if let Some(hex) = ref_name.strip_prefix("refs/visibleheads/") {
                    let should_delete = match HgId::from_hex(hex.as_bytes()) {
                        Ok(id) => {
                            git_visibleheads.insert(id);
                            !visibleheads.contains(&id)
                        }
                        _ => true,
                    };
                    if should_delete {
                        tracing::debug!(ref_name = &ref_name, "deleting visiblehead ref");
                        reference.delete()?;
                    }
                }
            }
            // Insert new visibleheads.
            for id in visibleheads.difference(&git_visibleheads) {
                let ref_name = format!("refs/visibleheads/{}", id.to_hex());
                let oid = hgid_to_git_oid(*id);
                tracing::debug!(ref_name = &ref_name, "adding visiblehead ref");
                repo.reference(&ref_name, oid, true, &reflog_message)?;
            }
        }

        // Incrementally update changed bookmarks, remotenames.
        'update_changes: {
            let parent = match metalog.parent()? {
                None => {
                    tracing::debug!("metalog parent is missing - skip updating changes");
                    break 'update_changes; // skip - no parent
                }
                Some(v) => v,
            };
            let old_bookmarks = parent.get_bookmarks()?;
            let old_remotenames = parent.get_remotenames()?;
            let new_bookmarks = metalog.get_bookmarks()?;
            let new_remotenames = metalog.get_remotenames()?;

            for (name, optional_id) in find_changes(&old_remotenames, &new_remotenames) {
                let ref_name = match name.split_once("/tags/") {
                    Some((remote, tag)) if !remote.contains('/') => {
                        format!("refs/remotetags/{}/{}", remote, tag)
                    }
                    _ => format!("refs/remotes/{}", name),
                };
                tracing::debug!(ref_name=&ref_name, id=?optional_id, "updating remotename ref");
                ref_to_change.insert(ref_name, optional_id.map(hgid_to_git_oid));
            }
            for (name, optional_id) in find_changes(&old_bookmarks, &new_bookmarks) {
                let ref_name = format!("refs/heads/{}", name);
                tracing::debug!(ref_name=&ref_name, id=?optional_id, "updating bookmark ref");
                ref_to_change.insert(ref_name, optional_id.map(hgid_to_git_oid));
            }

            if !ref_to_change.is_empty() {
                for reference in repo.references()? {
                    let mut reference = reference?;
                    let ref_name = match reference.name() {
                        Some(n) => n,
                        None => continue,
                    };
                    match ref_to_change.remove(ref_name) {
                        None => continue,
                        Some(None) => reference.delete()?,
                        Some(Some(oid)) => {
                            repo.reference(ref_name, oid, true, &reflog_message)?;
                        }
                    }
                }
                for (ref_name, optional_oid) in ref_to_change {
                    if let Some(oid) = optional_oid {
                        repo.reference(&ref_name, oid, true, &reflog_message)?;
                    }
                }
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
                    return Err(crate::errors::hash_mismatch(
                        &Vertex::copy_from(oid.as_ref()),
                        &commit.vertex,
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
        utils::add_graph_nodes_to_dag(&mut self.dag, graph_nodes).await
    }

    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        let heads = VertexListWithOptions::from(master_heads).with_desired_group(Group::MASTER);
        self.dag.flush(&heads).await?;
        Ok(())
    }

    async fn flush_commit_data(&mut self) -> Result<()> {
        Ok(())
    }

    fn update_references_to_match_metalog(&mut self, metalog: &MetaLog) -> Result<()> {
        self.export_to_git(metalog)
    }

    fn import_external_references(
        &mut self,
        metalog: &mut MetaLog,
        names: &[String],
    ) -> Result<()> {
        self.import_specified_refs_from_git(metalog, names)
    }

    async fn update_virtual_nodes(&mut self, wdir_parents: Vec<Vertex>) -> Result<()> {
        // For hg compatibility, use the same hardcoded hashes.
        let null = Vertex::from(HgId::null_id().as_ref());
        let wdir = Vertex::from(HgId::wdir_id().as_ref());
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
        Ok(())
    }
}

#[async_trait::async_trait]
impl ReadCommitText for GitSegmentedCommits {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let repo = self.git_repo.lock();
        get_commit_raw_text(&repo, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        ArcMutexGitRepo(self.git_repo.clone()).to_dyn_read_commit_text()
    }

    fn to_dyn_read_root_tree_ids(&self) -> Arc<dyn ReadRootTreeIds + Send + Sync> {
        // The default impl works. But ReadCommitText has overhead constructing
        // the hg text. Bypass that overhead.
        Arc::new(Wrapper(self.git_repo.clone()))
    }
}

#[derive(Clone)]
struct ArcMutexGitRepo(Arc<Mutex<git2::Repository>>);

#[async_trait::async_trait]
impl ReadCommitText for ArcMutexGitRepo {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let repo = self.0.lock();
        get_commit_raw_text(&repo, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }
}

fn get_commit_raw_text(repo: &git2::Repository, vertex: &Vertex) -> Result<Option<Bytes>> {
    let oid = match git2::Oid::from_bytes(vertex.as_ref()) {
        Ok(oid) => oid,
        Err(_) => return Ok(None),
    };
    let commit = match repo.find_commit(oid) {
        Ok(commit) => commit,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            return Ok(get_hard_coded_commit_text(vertex));
        }
        Err(e) => return Err(e.into()),
    };
    let text = to_hg_text(&commit);
    Ok(Some(text))
}

// Workaround orphan rule
struct Wrapper<T>(T);

#[async_trait::async_trait]
impl ReadRootTreeIds for Wrapper<Arc<Mutex<git2::Repository>>> {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let mut result = Vec::with_capacity(commits.len());
        let repo = self.0.lock();
        for commit_hgid in commits {
            if commit_hgid.is_null() {
                continue;
            }

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
        let git_repo = git2::Repository::open(self.git.git_dir())?;
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
        Err(io::Error::new(io::ErrorKind::Unsupported, "strip for git backend").into())
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
            self.git.git_dir().display(),
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

// For "Wed, 23 Nov 2022 17:47:30 -0800",
//
// git commit message: "1669254450 -0800"
// hg commit message:  "1669254450 28800"
// libgit2 Time::offset_minutes: -480

fn to_hg_date_text(time: &git2::Time) -> String {
    // See above. Convert -480 to 28800.
    let hg_date_offset_seconds = -time.offset_minutes() * 60;
    format!("{} {}", time.seconds(), hg_date_offset_seconds)
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

    let author = commit.author();
    let committer = commit.committer();

    // author
    write(utf8(author.name_bytes()).as_bytes());
    write(b" <");
    write(utf8(author.email_bytes()).as_bytes());
    write(b">\n");

    // date
    // We want the "modified" date to match user expectation. For hg we bump dates on commit
    // rewrites (rebase, metaedit, amend, ...). So the hg "date" is the "modified" date.
    // Usually, the committer date is the "modified" date. For tests, to preserve "stable"
    // hashes the "date.now" is patched to return UNIX epoch. So we pick the maximum date
    // from author and committer dates for test compatibility.
    let max_date = committer.when().max(author.when());
    write(to_hg_date_text(&max_date).as_bytes());

    // extras
    // The extras format is: "\0".join(f"{key}:{value}"). See "encodeextra" in changelog.py.
    write(b" author_date:");
    write(to_hg_date_text(&author.when()).as_bytes());
    write(b"\0committer:");
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

/// Find "deleted" and "changed" references.
fn find_changes<'a>(
    old: &'a BTreeMap<String, HgId>,
    new: &'a BTreeMap<String, HgId>,
) -> impl Iterator<Item = (&'a String, Option<HgId>)> + 'a {
    let deleted = old
        .keys()
        .filter(|name| !new.contains_key(name.as_str()))
        .map(|name| (name, None));
    let changed = new.iter().filter_map(|(name, value)| {
        let old_value = old.get(name.as_str());
        if old_value != Some(value) {
            Some((name, Some(*value)))
        } else {
            None
        }
    });
    deleted.chain(changed)
}

fn hgid_to_git_oid(id: HgId) -> git2::Oid {
    git2::Oid::from_bytes(id.as_ref()).expect("HgId should convert to git2::Oid")
}

/// Hardcoded commit hashes defined by hg.
fn get_hard_coded_commit_text(vertex: &Vertex) -> Option<Bytes> {
    let vertex = vertex.as_ref();
    if vertex == HgId::null_id().as_ref() || vertex == HgId::wdir_id().as_ref() {
        Some(Default::default())
    } else {
        None
    }
}
