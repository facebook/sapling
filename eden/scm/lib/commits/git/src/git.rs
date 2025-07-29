/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
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
use dag::Group;
use dag::Set;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::VertexOptions;
use dag::delegate;
use dag::errors::NotFoundError;
use dag::ops::DagPersistent;
use dag::ops::IdConvert;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use gitcompat::BareGit;
use gitcompat::GitCmd as _;
use gitcompat::ReferenceValue;
use gitdag::GitDag;
use gitstore::GitStore;
use gitstore::ObjectType;
use metalog::MetaLog;
use minibytes::Bytes;
use paste::paste;
use pathmatcher::Matcher;
use pathmatcher::TreeMatcher;
use refencode::RefName;
use spawn_ext::CommandError;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use types::HgId;
use types::fetch_mode::FetchMode;

use crate::ref_filter::GitRefMetaLogFilter;
use crate::ref_matcher::GitRefPreliminaryMatcher;
use crate::utils;

/// Git commits with segments index.
///
/// Source of truth is `.git/`. This struct bi-directionally syncs
/// the `.git/` commit graph with Sapling's own structures like
/// segmented changelog and metalog. Commit messages are not double
/// stored and will be read from `.git/` directly.
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
    git_store: GitStore,
    dag: GitDag,
    dag_path: PathBuf,
    git: BareGit,
    // Read from config
    relevant_config: RelevantConfig,
}

enum RelevantConfig {
    /// ".sl" mode does not need those configs.
    DotSl,
    /// ".git" mode need those configs.
    DotGit {
        hoist: Option<Text>,
        /// Matched remote refs (ex. "main") will be imported.
        /// Implicit ref prefix: refs/remotes/{something}/
        selective_pull_default: HashSet<String>,
        /// Matched remote refs (ex. "m/*") will be imported.
        /// Implicit ref prefix: refs/remotes/
        import_remote_refs: Option<Box<dyn Matcher + Send + Sync>>,
    },
}

impl DagCommits for GitSegmentedCommits {}

impl GitSegmentedCommits {
    pub fn new(
        git_dir: &Path,
        dag_dir: &Path,
        config: &dyn Config,
        is_dotgit: bool,
    ) -> Result<Self> {
        let git_store = GitStore::open(git_dir, config)?;
        let dag = GitDag::open(dag_dir)?;
        let dag_path = dag_dir.to_path_buf();
        let git_path = git_dir.to_path_buf();
        let git = BareGit::from_git_dir_and_config(git_path, config);
        let relevant_config = if is_dotgit {
            let hoist = config.get("remotenames", "hoist");
            let selective_pull_default =
                config.get_or_default("remotenames", "selectivepulldefault")?;
            let import_remote_refs = config
                .get_opt::<TreeMatcher>("git", "import-remote-refs")?
                .map(|v| Box::new(v) as Box<dyn Matcher + Send + Sync>);
            RelevantConfig::DotGit {
                hoist,
                selective_pull_default,
                import_remote_refs,
            }
        } else {
            RelevantConfig::DotSl
        };
        Ok(Self {
            git_store,
            dag,
            dag_path,
            git,
            relevant_config,
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

        let matcher = GitRefPreliminaryMatcher::new();

        let refs: BTreeMap<String, ReferenceValue> = self.git.list_references(Some(&matcher))?;
        let head_id = self.git.resolve_head()?;

        // Bookmarks and remotenames are built from scratch.
        let mut bookmarks = BTreeMap::new();
        let mut remotenames = BTreeMap::new();
        let mut extra_git_refs = BTreeMap::new();
        let mut visibleheads = Vec::new();

        let existing_visibleheads: HashSet<_> = metalog.get_visibleheads()?.into_iter().collect();
        let existing_remotenames = metalog.get_remotenames()?;
        let existing_bookmarks = metalog.get_bookmarks()?;

        // Heads (vertexes) to import to dag.
        let mut heads = Vec::new();
        let head_opts = VertexOptions::default();

        let remote_name_filter: GitRefMetaLogFilter = match &self.relevant_config {
            RelevantConfig::DotGit {
                hoist,
                selective_pull_default,
                import_remote_refs,
            } => GitRefMetaLogFilter::new_for_dotgit(
                &refs,
                &existing_remotenames,
                hoist.as_deref(),
                selective_pull_default,
                import_remote_refs.as_deref(),
                head_id,
            )?,
            RelevantConfig::DotSl => GitRefMetaLogFilter::new_for_dotsl(&refs)?,
        };

        for (ref_name, value) in &refs {
            // Resolve symlink refs.
            let mut id = match value {
                ReferenceValue::Sym(name) => match self.git.lookup_reference_follow_links(name)? {
                    None => continue,
                    Some(id) => id,
                },
                ReferenceValue::Id(id) => *id,
            };
            let names: Vec<&str> = ref_name.splitn(3, '/').collect();
            match &names[..] {
                ["refs", "remotes", name] => {
                    if remote_name_filter.should_import_remote_name(name, &id)? {
                        let mut should_import_to_dag = match existing_remotenames.get(*name) {
                            Some(&existing_id) => existing_id != id,
                            None => true,
                        };
                        if should_import_to_dag {
                            // Resolve tags recursively.
                            // Metalog references should not contain tags. Most users of metalog
                            // expect references to be commits.
                            let mut is_tag = false;
                            while let Some(tag_data) = self
                                .git_store
                                .read_local_obj_optional(id, ObjectType::Tag)?
                            {
                                if let Some(target_id) = format_util::resolve_git_tag(&tag_data) {
                                    is_tag = true;
                                    id = target_id;
                                } else {
                                    break;
                                }
                            }

                            // If `id` is changed because of tag, re-calculate `should_import_to_dag`.
                            // We calculate `should_import_to_dag` first before reading tags, to
                            // reduce overhead reading (known) non-tag objects.
                            if is_tag {
                                should_import_to_dag = match existing_remotenames.get(*name) {
                                    Some(&existing_id) => existing_id != id,
                                    None => true,
                                };
                            }

                            if should_import_to_dag {
                                let mut opts = head_opts.clone();
                                if remote_name_filter.is_main_remote_name(name) {
                                    opts.desired_group = Group::MASTER;
                                }
                                heads.push((Vertex::copy_from(id.as_ref()), opts));
                            }
                        }
                        remotenames.insert(RefName::try_from(*name)?, id);
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
                    remotenames.insert(RefName::try_from(remotename)?, id);
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
                        extra_git_refs.insert(RefName::try_from(ref_name.as_str())?, id);
                        visibleheads.push(id);
                    } else {
                        bookmarks.insert(RefName::try_from(*name)?, id);
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
            ?extra_git_refs,
            ?existing_remotenames,
            ?existing_bookmarks,
            ?existing_visibleheads,
            ?heads,
            "git import"
        );

        self.dag
            .import_from_git(&self.git_store, heads, self.git.git_dir())?;

        let encoded_bookmarks = refencode::encode_bookmarks(&bookmarks);
        let encoded_remotenames = refencode::encode_remotenames(&remotenames);
        let encoded_visibleheads = refencode::encode_visibleheads(&visibleheads);
        metalog.set("bookmarks", encoded_bookmarks.as_ref())?;
        metalog.set("remotenames", encoded_remotenames.as_ref())?;
        metalog.set("visibleheads", encoded_visibleheads.as_ref())?;
        metalog.set_git_refs(&extra_git_refs)?;

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
            bookmarks: Option<BTreeMap<RefName, HgId>>,
            remotenames: Option<BTreeMap<RefName, HgId>>,
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
                    fn [<update_ $field>](&mut self, metalog: &MetaLog, name: RefName, value: Option<HgId>) -> Result<()> {
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
                state.update_bookmarks(metalog, RefName::try_from(name)?, ref_value)?;
            } else if let Some(name) = ref_name.strip_prefix("refs/remotes/") {
                state.update_remotenames(metalog, RefName::try_from(name)?, ref_value)?;
            } else if let Some(rest) = ref_name.strip_prefix("refs/remotetags/") {
                let name = match rest.split_once('/') {
                    Some((remote, name)) => format!("{}/tags/{}", remote, name),
                    None => bail!("illformed ref_name: {}", ref_name),
                };
                state.update_remotenames(metalog, RefName::try_from(name)?, ref_value)?;
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
            self.dag
                .import_from_git(&self.git_store, heads.into(), self.git.git_dir())?;
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
        let mut ref_to_change = HashMap::<String, Option<HgId>>::new();

        let new_bookmarks = metalog.get_bookmarks()?;
        let new_git_refs = metalog.get_git_refs()?;
        let new_remotenames = metalog.get_remotenames()?;

        let new_visible_oids: HashSet<_> = new_bookmarks
            .values()
            .chain(new_git_refs.values())
            .chain(new_remotenames.values())
            .collect();

        // Update visibleheads in refs/visibleheads/.
        {
            let visibleheads = metalog.get_visibleheads()?;
            let visibleheads: HashSet<HgId> = visibleheads.into_iter().collect();
            let mut git_visibleheads = HashSet::with_capacity(visibleheads.len());
            // Delete non-existed visibleheads.
            for (ref_name, _ref_value) in self.git.list_references(None)? {
                if let Some(hex) = ref_name.strip_prefix("refs/visibleheads/") {
                    let should_delete = match HgId::from_hex(hex.as_bytes()) {
                        Ok(id) => {
                            git_visibleheads.insert(id);
                            !visibleheads.contains(&id)
                        }
                        _ => true,
                    };
                    if should_delete {
                        tracing::trace!(ref_name, "removing visiblehead");
                        ref_to_change.insert(ref_name.to_string(), None);
                    }
                }
            }
            // Insert new visibleheads.
            for id in visibleheads.difference(&git_visibleheads) {
                if new_visible_oids.contains(id) {
                    tracing::trace!(?id, "skipping visiblehead - matches another ref");
                } else {
                    let ref_name = format!("refs/visibleheads/{}", id.to_hex());
                    tracing::trace!(ref_name, ?id, "setting visiblehead");
                    ref_to_change.insert(ref_name, Some(*id));
                }
            }
        }

        // Incrementally update changed bookmarks, remotenames, git_refs.
        'update_changes: {
            let parent = match metalog.parent()? {
                None => {
                    tracing::debug!(
                        "metalog parent is missing - skip updating non-visiblehead refs"
                    );
                    break 'update_changes; // skip - no parent
                }
                Some(v) => v,
            };
            let old_bookmarks = parent.get_bookmarks()?;
            let old_remotenames = parent.get_remotenames()?;
            let old_git_refs = parent.get_git_refs()?;

            for (name, optional_id) in find_changes(&old_remotenames, &new_remotenames) {
                let ref_name = match name.split_once("/tags/") {
                    Some((remote, tag)) if !remote.contains('/') => {
                        format!("refs/remotetags/{}/{}", remote, tag)
                    }
                    _ => format!("refs/remotes/{}", name),
                };
                tracing::trace!(ref_name=&ref_name, id=?optional_id, "updating remotename ref");
                ref_to_change.insert(ref_name, optional_id);
            }
            for (name, optional_id) in find_changes(&old_bookmarks, &new_bookmarks) {
                let ref_name = format!("refs/heads/{}", name);
                tracing::trace!(ref_name=&ref_name, id=?optional_id, "updating bookmark ref");
                ref_to_change.insert(ref_name, optional_id);
            }
            for (ref_name, optional_id) in find_changes(&old_git_refs, &new_git_refs) {
                tracing::trace!(ref_name=ref_name.as_str(), id=?optional_id, "updating git ref");
                ref_to_change.insert(ref_name.to_string(), optional_id);
            }
        }

        // Run `git update-ref` to apply updates.
        if !ref_to_change.is_empty() {
            tracing::debug!(?ref_to_change, "updating git ref");

            let mut update_ref_stdin = String::new();
            for (name, value) in ref_to_change {
                match value {
                    Some(oid) => {
                        update_ref_stdin.push_str(&format!(
                            "update {}\0{}\0\0",
                            name,
                            oid.to_hex()
                        ));
                    }
                    None => {
                        update_ref_stdin.push_str(&format!("delete {}\0\0", name));
                    }
                }
            }
            let mut cmd = self.git.git_cmd(
                "update-ref",
                &[
                    "-m",
                    reflog_message.as_str(),
                    "--no-deref",
                    "--create-reflog",
                    "--stdin",
                    "-z",
                ],
            );
            let mut child = cmd.stdin(Stdio::piped()).spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(update_ref_stdin.as_bytes())?;
                drop(stdin);
            }
            let status = child.wait()?;
            if !status.success() {
                let err = CommandError::new(&cmd, None).with_status(&status);
                return Err(err.into());
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
            for commit in commits {
                let oid = self
                    .git_store
                    .write_obj(ObjectType::Commit, commit.raw_text.as_ref())?;
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
        get_commit_raw_text(&self.git_store, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Wrapper(self.git_store.clone()).to_dyn_read_commit_text()
    }

    fn to_dyn_read_root_tree_ids(&self) -> Arc<dyn ReadRootTreeIds + Send + Sync> {
        // The default impl works. But ReadCommitText has overhead constructing
        // the hg text. Bypass that overhead.
        Arc::new(Wrapper(self.git_store.clone()))
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }
}

#[async_trait::async_trait]
impl ReadCommitText for Wrapper<GitStore> {
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        get_commit_raw_text(&self.0, vertex)
    }

    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync> {
        Arc::new(self.clone())
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }
}

fn get_commit_raw_text(store: &GitStore, vertex: &Vertex) -> Result<Option<Bytes>> {
    let id = match HgId::from_slice(vertex.as_ref()) {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };
    // Git commits are not expected to be lazily fetched here.
    match store.read_local_obj_optional(id, ObjectType::Commit)? {
        None => Ok(get_hard_coded_commit_text(vertex)),
        Some(data) => Ok(Some(Bytes::from(data))),
    }
}

// Workaround orphan rule
#[derive(Clone)]
struct Wrapper<T>(T);

#[async_trait::async_trait]
impl ReadRootTreeIds for Wrapper<GitStore> {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let mut result = Vec::with_capacity(commits.len());
        for commit_id in commits {
            if commit_id.is_null() {
                continue;
            }
            let data = self
                .0
                .read_obj(commit_id, ObjectType::Commit, FetchMode::LocalOnly)?;
            let data = Bytes::from(data);
            let text = data.into_text_lossy();
            let fields = format_util::commit_text_to_fields(text, SerializationFormat::Git);
            let tree_id = fields.root_tree()?;
            result.push((commit_id, tree_id));
        }
        Ok(result)
    }
}

impl StreamCommitText for GitSegmentedCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        let store = self.git_store.clone();
        let stream = stream.map(move |item| {
            let vertex = item?;
            let raw_text = match get_commit_raw_text(&store, &vertex)? {
                Some(v) => v,
                None => return vertex.not_found().map_err(Into::into),
            };
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

/// Find "deleted" and "changed" references.
fn find_changes<'a>(
    old: &'a BTreeMap<RefName, HgId>,
    new: &'a BTreeMap<RefName, HgId>,
) -> impl Iterator<Item = (&'a RefName, Option<HgId>)> + 'a {
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

/// Hardcoded commit hashes defined by hg.
fn get_hard_coded_commit_text(vertex: &Vertex) -> Option<Bytes> {
    let vertex = vertex.as_ref();
    if vertex == HgId::null_id().as_ref() || vertex == HgId::wdir_id().as_ref() {
        Some(Default::default())
    } else {
        None
    }
}
