/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod trie;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use changeset_fetcher::ArcChangesetFetcher;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::AclRegion;
use metaconfig_types::AclRegionConfig;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use reachabilityindex::ReachabilityIndex;
use std::collections::HashMap;
use std::sync::Arc;

use trie::PrefixTrieWithRules;

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AclRegionRuleDescriptor {
    pub name: String,
    pub hipster_acl: String,
}

pub enum AssociatedRulesResult {
    AclRegionsDisabled,
    Rules(Vec<AclRegionRuleDescriptor>),
}

#[async_trait]
#[facet::facet]
pub trait AclRegions: Send + Sync {
    async fn associated_rules(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        path: Option<&MPath>,
    ) -> Result<AssociatedRulesResult>;
}

struct AclRegionsImpl {
    path_rules_index: PrefixTrieWithRules,
    reachability_index: Arc<dyn ReachabilityIndex>,
    changeset_fetcher: ArcChangesetFetcher,
}

impl AclRegionsImpl {
    fn new(
        config: &AclRegionConfig,
        reachability_index: Arc<dyn ReachabilityIndex>,
        changeset_fetcher: ArcChangesetFetcher,
    ) -> AclRegionsImpl {
        let mut path_rules_index = PrefixTrieWithRules::new();
        for rule in &config.allow_rules {
            path_rules_index.add_rule(Arc::new(rule.clone()));
        }
        AclRegionsImpl {
            path_rules_index,
            reachability_index,
            changeset_fetcher,
        }
    }

    async fn is_commit_descendant_of_any(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        candidates: &[ChangesetId],
    ) -> Result<bool> {
        let mut is_descendant_results = stream::iter(candidates)
            .map(|candidate| {
                self.reachability_index.query_reachability(
                    ctx,
                    &self.changeset_fetcher,
                    cs_id,
                    *candidate,
                )
            })
            .boxed()
            .buffered(10);

        while let Some(is_descendant) = is_descendant_results.try_next().await? {
            if is_descendant {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn is_commit_in_region(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        region: &AclRegion,
    ) -> Result<bool> {
        let is_any_root_descendant = region.roots.is_empty()
            || self
                .is_commit_descendant_of_any(ctx, cs_id, &region.roots)
                .await?;
        let is_any_head_descendant = self
            .is_commit_descendant_of_any(ctx, cs_id, &region.heads)
            .await?;
        Ok(is_any_root_descendant && !is_any_head_descendant)
    }
}

#[async_trait]
impl AclRegions for AclRegionsImpl {
    async fn associated_rules(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        path: Option<&MPath>,
    ) -> Result<AssociatedRulesResult> {
        let matched_rules: HashMap<_, _> =
            stream::iter(self.path_rules_index.associated_rules(path))
                .map(|((name, region_index), rule)| async move {
                    let region = &rule.regions.get(region_index.0).ok_or_else(|| {
                        anyhow!(
                            "Incorrect region index {} for rule {}",
                            region_index.0,
                            rule.name
                        )
                    })?;
                    anyhow::Ok(
                        self.is_commit_in_region(ctx, cs_id, region)
                            .await?
                            .then(|| (name, rule)),
                    )
                })
                .buffered(10)
                .filter_map(|rule| async { rule.transpose() })
                .try_collect()
                .await?;

        Ok(AssociatedRulesResult::Rules(
            matched_rules
                .into_iter()
                .map(|(name, rule)| AclRegionRuleDescriptor {
                    name,
                    hipster_acl: rule.hipster_acl.clone(),
                })
                .collect(),
        ))
    }
}

struct DisabledAclRegions {}

#[async_trait]
impl AclRegions for DisabledAclRegions {
    async fn associated_rules(
        &self,
        _ctx: &CoreContext,
        _cs_id: ChangesetId,
        _path: Option<&MPath>,
    ) -> Result<AssociatedRulesResult> {
        Ok(AssociatedRulesResult::AclRegionsDisabled)
    }
}

pub fn build_acl_regions(
    config: Option<&AclRegionConfig>,
    reachability_index: Arc<dyn ReachabilityIndex>,
    changeset_fetcher: ArcChangesetFetcher,
) -> ArcAclRegions {
    match config {
        Some(config) => Arc::new(AclRegionsImpl::new(
            config,
            reachability_index,
            changeset_fetcher,
        )),
        None => build_disabled_acl_regions(),
    }
}

pub fn build_disabled_acl_regions() -> ArcAclRegions {
    Arc::new(DisabledAclRegions {})
}
