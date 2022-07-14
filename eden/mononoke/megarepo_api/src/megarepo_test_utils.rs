/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use commit_transformation::create_source_to_target_multi_mover;
use context::CoreContext;
use maplit::btreemap;
use megarepo_config::MergeMode;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::SourceRevision;
use megarepo_config::SyncConfigVersion;
use megarepo_config::Target;
use megarepo_config::TestMononokeMegarepoConfigs;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::MegarepoMapping;
use megarepo_mapping::Source;
use megarepo_mapping::SourceMappingRules;
use megarepo_mapping::SourceName;
use megarepo_mapping::SyncTargetConfig;
use mononoke_api::Mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mutable_renames::MutableRenames;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::list_working_copy_utf8;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;

pub struct MegarepoTest {
    pub blobrepo: BlobRepo,
    pub megarepo_mapping: Arc<MegarepoMapping>,
    pub mononoke: Arc<Mononoke>,
    pub configs_storage: TestMononokeMegarepoConfigs,
    pub mutable_renames: Arc<MutableRenames>,
}

impl MegarepoTest {
    pub async fn new(ctx: &CoreContext) -> Result<Self, Error> {
        let id = RepositoryId::new(0);
        let mut factory = TestRepoFactory::new(ctx.fb)?;
        factory.with_id(id);
        let megarepo_mapping = factory.megarepo_mapping();
        let config = factory.repo_config();
        let repo_identity = factory.repo_identity(&config);
        let mutable_renames = factory.mutable_renames(&repo_identity)?;
        let blobrepo: BlobRepo = factory.build()?;
        let mononoke = Arc::new(
            Mononoke::new_test(ctx.clone(), vec![("repo".to_string(), blobrepo.clone())]).await?,
        );
        let configs_storage = TestMononokeMegarepoConfigs::new(ctx.logger());

        Ok(Self {
            blobrepo,
            megarepo_mapping,
            mononoke,
            configs_storage,
            mutable_renames,
        })
    }

    pub fn repo_id(&self) -> RepositoryId {
        self.blobrepo.get_repoid()
    }

    pub fn target(&self, bookmark: String) -> Target {
        Target {
            repo_id: self.repo_id().id() as i64,
            bookmark,
        }
    }

    pub async fn prepare_initial_commit_in_target(
        &self,
        ctx: &CoreContext,
        version: &SyncConfigVersion,
        target: &Target,
    ) -> Result<ChangesetId, Error> {
        let initial_config = self.configs_storage.get_config_by_version(
            ctx.clone(),
            target.clone(),
            version.clone(),
        )?;

        let mut init_target_cs = CreateCommitContext::new_root(ctx, &self.blobrepo);

        let mut remapping_state = btreemap! {};
        for source in initial_config.sources {
            let mover = create_source_to_target_multi_mover(source.mapping.clone())?;
            let init_source_cs_id = match source.revision {
                SourceRevision::bookmark(bookmark) => {
                    resolve_cs_id(ctx, &self.blobrepo, bookmark).await?
                }
                SourceRevision::hash(hash) => {
                    let cs_id = ChangesetId::from_bytes(hash)?;
                    resolve_cs_id(ctx, &self.blobrepo, cs_id).await?
                }
                _ => {
                    unimplemented!()
                }
            };
            let source_wc = list_working_copy_utf8(ctx, &self.blobrepo, init_source_cs_id).await?;

            for (file, content) in source_wc {
                let target_files = mover(&file)?;
                for target_file in target_files {
                    init_target_cs = init_target_cs.add_file(target_file, content.clone());
                }
            }
            remapping_state.insert(SourceName::new(source.source_name), init_source_cs_id);
        }

        let mut init_target_cs = init_target_cs.create_commit_object().await?;
        let remapping_state =
            CommitRemappingState::new(remapping_state, initial_config.version.clone());
        remapping_state
            .save_in_changeset(ctx, &self.blobrepo, &mut init_target_cs)
            .await?;
        let init_target_cs = init_target_cs.freeze()?;
        let init_target_cs_id = init_target_cs.get_changeset_id();
        blobrepo::save_bonsai_changesets(vec![init_target_cs], ctx.clone(), &self.blobrepo).await?;

        bookmark(ctx, &self.blobrepo, target.bookmark.clone())
            .set_to(init_target_cs_id)
            .await?;
        Ok(init_target_cs_id)
    }
}

pub struct SyncTargetConfigBuilder {
    repo_id: RepositoryId,
    target: Target,
    version: SyncConfigVersion,
    sources: Vec<Source>,
}

impl SyncTargetConfigBuilder {
    pub fn new(repo_id: RepositoryId, target: Target, version: SyncConfigVersion) -> Self {
        Self {
            repo_id,
            target,
            version,
            sources: vec![],
        }
    }

    pub fn source_builder(self, source_name: SourceName) -> SourceVersionBuilder {
        SourceVersionBuilder::new(source_name, self.repo_id, self)
    }

    pub fn add_source(&mut self, source: Source) {
        self.sources.push(source)
    }

    pub fn build(self, configs_storage: &mut TestMononokeMegarepoConfigs) {
        let (target, version) = (self.target.clone(), self.version.clone());
        let config = self.no_storage_build();
        configs_storage.add((target, version), config);
    }

    pub fn no_storage_build(self) -> SyncTargetConfig {
        SyncTargetConfig {
            target: self.target,
            sources: self.sources,
            version: self.version,
        }
    }
}

pub struct SourceVersionBuilder {
    source_name: SourceName,
    git_repo_name: String,
    default_prefix: Option<String>,
    source_bookmark: Option<String>,
    source_changeset: Option<ChangesetId>,
    repo_id: RepositoryId,
    config_builder: SyncTargetConfigBuilder,
    linkfiles: BTreeMap<String, String>,
    copyfiles: BTreeMap<String, String>,
    merge_mode: Option<MergeMode>,
}

impl SourceVersionBuilder {
    pub fn new(
        source_name: SourceName,
        repo_id: RepositoryId,
        config_builder: SyncTargetConfigBuilder,
    ) -> Self {
        Self {
            source_name: source_name.clone(),
            // This field won't be used much in tests, so just set to the same
            // value as source_name
            git_repo_name: source_name.to_string(),
            default_prefix: None,
            source_bookmark: None,
            source_changeset: None,
            repo_id,
            config_builder,
            linkfiles: BTreeMap::new(),
            copyfiles: BTreeMap::new(),
            merge_mode: None,
        }
    }

    pub fn set_prefix_bookmark_to_source_name(mut self) -> Self {
        self.default_prefix = Some(self.source_name.0.clone());
        self.source_bookmark = Some(self.source_name.0.clone());
        self
    }

    #[allow(unused)]
    pub fn default_prefix(mut self, default_prefix: impl ToString) -> Self {
        self.default_prefix = Some(default_prefix.to_string());
        self
    }

    #[allow(unused)]
    pub fn bookmark(mut self, bookmark: impl ToString) -> Self {
        self.source_bookmark = Some(bookmark.to_string());
        self
    }

    pub fn source_changeset(mut self, cs_id: ChangesetId) -> Self {
        self.source_changeset = Some(cs_id);
        self
    }

    pub fn linkfile<S1: ToString, S2: ToString>(mut self, src: S1, dst: S2) -> Self {
        self.linkfiles.insert(dst.to_string(), src.to_string());
        self
    }

    #[allow(dead_code)]
    pub fn copyfile<S1: ToString, S2: ToString>(mut self, src: S1, dst: S2) -> Self {
        self.copyfiles.insert(src.to_string(), dst.to_string());
        self
    }

    pub fn merge_mode(mut self, mode: MergeMode) -> Self {
        self.merge_mode = Some(mode);
        self
    }

    pub fn build_source(mut self) -> Result<SyncTargetConfigBuilder, Error> {
        let source_revision = match (self.source_bookmark, self.source_changeset) {
            (Some(_), Some(_)) => {
                return Err(anyhow!("both source bookmark and changeset are specified"));
            }
            (Some(source_bookmark), None) => SourceRevision::bookmark(source_bookmark),
            (None, Some(source_changeset)) => {
                SourceRevision::hash(Vec::from(source_changeset.as_ref()))
            }
            (None, None) => {
                return Err(anyhow!(
                    "neither source bookmark nor source commit were set"
                ));
            }
        };

        let default_prefix = self
            .default_prefix
            .ok_or_else(|| anyhow!("default prefix is not set"))?;

        let mut overrides = BTreeMap::new();
        for (src, dest) in self.copyfiles.into_iter() {
            let src_root_relative = Path::new(&default_prefix).join(&src);
            overrides.insert(
                src.clone(),
                vec![src_root_relative.to_str().unwrap().to_string(), dest],
            );
        }
        let source = Source {
            source_name: self.source_name.to_string(),
            repo_id: self.repo_id.id() as i64,
            name: self.git_repo_name,
            revision: source_revision,
            mapping: SourceMappingRules {
                default_prefix,
                linkfiles: self.linkfiles,
                overrides,
            },
            merge_mode: self.merge_mode,
        };
        self.config_builder.add_source(source);
        Ok(self.config_builder)
    }
}
