/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use megarepo_configs::types::SyncTargetConfig;
use slog::warn;
use std::collections::HashSet;

/// Verify the config
pub fn verify_config(ctx: &CoreContext, config: &SyncTargetConfig) -> Result<()> {
    verify_unique_source_names(ctx, config)
}

fn verify_unique_source_names(ctx: &CoreContext, config: &SyncTargetConfig) -> Result<()> {
    let mut seen = HashSet::new();
    let mut seen_more_than_once = HashSet::new();
    config.sources.iter().for_each(|src| {
        if !seen.insert(&src.source_name) {
            seen_more_than_once.insert(&src.source_name);
        }
    });

    if !seen_more_than_once.is_empty() {
        warn!(
            ctx.logger(),
            "SyncTargetConfig validation error: non-unique source names: {:?}", seen_more_than_once
        );

        Err(anyhow!(
            "Non-unique source names: {:?}",
            seen_more_than_once
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use megarepo_configs::types::MergeMode;
    use megarepo_configs::types::Source;
    use megarepo_configs::types::SourceMappingRules;
    use megarepo_configs::types::SourceRevision;
    use megarepo_configs::types::Target;
    use megarepo_configs::types::WithExtraMoveCommit;

    fn s(v: &str) -> String {
        v.to_owned()
    }

    fn get_good_cfg() -> SyncTargetConfig {
        SyncTargetConfig {
            target: Target {
                repo_id: 1,
                bookmark: s("target"),
            },
            version: s("version"),
            sources: vec![
                Source {
                    name: s("name1"),
                    source_name: s("source1"),
                    revision: SourceRevision::bookmark(s("hello")),
                    repo_id: 1,
                    mapping: SourceMappingRules {
                        default_prefix: s("pre/fix1"),
                        linkfiles: btreemap! {
                            s("link/source_1") => s("link_target_1")
                        },
                        overrides: btreemap! {
                            s("deleted") => vec![],
                            s("multiplied1") => vec![s("multiplied1_1"), s("multiplied1_2")],
                            s("copyfile") => vec![
                                s("pre/fix1/copyfile"),
                                s("copiedfile"),
                            ]
                        },
                    },
                    merge_mode: Some(MergeMode::with_move_commit(WithExtraMoveCommit {
                        ..Default::default()
                    })),
                },
                Source {
                    name: s("name2"),
                    source_name: s("source2"),
                    revision: SourceRevision::bookmark(s("hello")),
                    repo_id: 1,
                    mapping: SourceMappingRules {
                        default_prefix: s("pre/fix2"),
                        linkfiles: btreemap! {
                            s("link/source_2") => s("link_target_2")
                        },
                        overrides: btreemap! {
                            s("deleted") => vec![],
                            s("multiplied2") => vec![s("multiplied2_1"), s("multiplied2_2")],
                        },
                    },
                    merge_mode: None,
                },
            ],
        }
    }

    #[fbinit::test]
    fn test_verify_unique_source_names(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let cfg = get_good_cfg();
        assert!(verify_unique_source_names(&ctx, &cfg).is_ok());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_unique_source_names_bad(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let mut cfg = get_good_cfg();
        cfg.sources[1].source_name = s("source1");
        assert!(verify_unique_source_names(&ctx, &cfg).is_err());

        Ok(())
    }
}
