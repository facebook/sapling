/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Context, Result};
use context::CoreContext;
use itertools::Itertools;
use megarepo_configs::types::SyncTargetConfig;
use mononoke_types::MPath;
use slog::warn;
use std::collections::HashSet;

/// A file in the target, where megarepo tooling
/// can store some metadata
pub const MEGAREPO_SPECIAL_PATH: &str = ".megarepo";

/// Verify the config
pub fn verify_config(ctx: &CoreContext, config: &SyncTargetConfig) -> Result<()> {
    verify_unique_source_names(ctx, config)?;
    verify_paths(ctx, config)
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

/// Check that the set of all relevant target paths in this configuration
/// is prefix free.
/// This is to ensure non-intersection of source images in the target repo.
/// If we were to allow such intersections, we may end up having changesets
/// originated in different sources modifying the same file in the target repo.
/// This would lead to conflicts, so we need to avoid that.
fn verify_paths(ctx: &CoreContext, config: &SyncTargetConfig) -> Result<()> {
    let default_prefixes = config
        .sources
        .iter()
        .map(|src| MPath::new(&src.mapping.default_prefix))
        .collect::<Result<Vec<MPath>>>()
        .context("failed parsing default prefixes into MPaths")?;

    let overrides_vec_vec = config
        .sources
        .iter()
        .map(|src| {
            let default_prefix = MPath::new(&src.mapping.default_prefix)?;

            // We need to filter out the overrides that map the file to the same
            // destination file as if override didn't exist e.g. overrides like
            // "fileA" -> "default_prefix/fileA".
            // These overrides happen in practice for e.g. copyfile repo manifest attribute
            // (https://gerrit.googlesource.com/git-repo/+/master/docs/manifest-format.md#Element-copyfile)
            // converts into override with two destination - one is the copy destination, and another
            // is the original place of the file.
            //
            // We need to filter them out since they are not prefix free with
            // default prefix, but we don't want to return error in this case.
            let mut to_check = vec![];
            for (override_src, override_dests) in &src.mapping.overrides {
                let src = default_prefix.join(&MPath::new(override_src)?);
                for override_dest in override_dests {
                    let override_dest = MPath::new(override_dest)?;
                    if src != override_dest {
                        to_check.push(override_dest);
                    }
                }
            }

            Ok(to_check)
        })
        .collect::<Result<Vec<Vec<MPath>>>>()
        .context("failed parsing overrides into MPaths")?;
    let overrides = overrides_vec_vec
        .into_iter()
        .flat_map(|vec| vec)
        .collect::<Vec<MPath>>();

    let linkfiles = config
        .sources
        .iter()
        .flat_map(|src| src.mapping.linkfiles.iter().map(|(_, rhs)| MPath::new(rhs)))
        .collect::<Result<Vec<MPath>>>()
        .context("failed parsing linkfiles into MPaths")?;

    let dot_megarepo = MPath::new(MEGAREPO_SPECIAL_PATH)
        .context("Failed to parse MEGAREPO_SPECIAL_PATH ¯\\_(ツ)_/¯")?;

    let all = {
        let mut all = Vec::new();
        all.extend(default_prefixes);
        all.extend(overrides);
        all.extend(linkfiles);
        all.push(dot_megarepo);
        all
    };

    print!("All: {:?}", all);

    verify_prefix_free(ctx, all).context("failed verifying that the config is prefix-free")
}

fn verify_prefix_free(ctx: &CoreContext, paths: Vec<MPath>) -> Result<()> {
    for (first_pfx, second_pfx) in paths.iter().tuple_combinations::<(_, _)>() {
        if first_pfx.is_prefix_of(second_pfx) || second_pfx.is_prefix_of(first_pfx) {
            warn!(
                ctx.logger(),
                "SyncTargetConfig validation error: {} and {} are not prefix-free",
                first_pfx,
                second_pfx,
            );
            return Err(anyhow!(
                "{} and {} are not prefix-free",
                first_pfx,
                second_pfx,
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use megarepo_configs::types::{Source, SourceMappingRules, SourceRevision, Target};

    fn mp(s: &str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn s(v: &str) -> String {
        v.to_owned()
    }

    #[fbinit::test]
    fn test_verify_prefix_free(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        assert!(verify_prefix_free(&ctx, vec!(mp("a/b/c"), mp("a/b/d"), mp("d"))).is_ok());
        assert!(verify_prefix_free(&ctx, vec!(mp("a/b/c"), mp("a/b/d"), mp("a"))).is_err());
        Ok(())
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
                },
            ],
        }
    }

    #[fbinit::test]
    fn test_verify_paths_good(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let cfg = get_good_cfg();
        assert!(verify_paths(&ctx, &cfg).is_ok());
        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_same_source_overrides(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.overrides = btreemap! {
            s("a") => vec![s("a"), s("a/b")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.overrides = btreemap! {
            s("a") => vec![s("a/b"), s("a")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.overrides = btreemap! {
            s("a") => vec![s("a/b"), s("a/c")],
            s("b") => vec![s("a")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_cross_source_overrides(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.overrides = btreemap! {
            s("a") => vec![s("a/b"), s("a/c")],
            s("b") => vec![s("b")],
        };
        cfg.sources[1].mapping.overrides = btreemap! {
            s("a") => vec![s("a"), s("a/d")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_same_source_linkfiles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.linkfiles = btreemap! {
            s("a") => s("c"),
            s("b") => s("c/d"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_cross_source_linkfiles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.linkfiles = btreemap! {
            s("a") => s("c"),
        };
        cfg.sources[1].mapping.linkfiles = btreemap! {
            s("b") => s("c/d"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_default_vs_overrides(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        // same source
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.overrides = btreemap! {
            s("a") => vec![s("pre/fix1/what")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        // cross source
        let mut cfg = get_good_cfg();
        cfg.sources[1].mapping.overrides = btreemap! {
            s("a") => vec![s("pre/fix1/what")],
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_default_vs_linkfiles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        // same source
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.linkfiles = btreemap! {
            s("a") => s("pre/fix1/what"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        // cross source
        let mut cfg = get_good_cfg();
        cfg.sources[1].mapping.linkfiles = btreemap! {
            s("a") => s("pre/fix1/what"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_overrides_vs_linkfiles(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        // same source
        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.linkfiles = btreemap! {
            s("a") => s("multiplied1_2"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        // cross source
        let mut cfg = get_good_cfg();
        cfg.sources[1].mapping.linkfiles = btreemap! {
            s("a") => s("multiplied1_2"),
        };
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_cross_source_default(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut cfg = get_good_cfg();

        cfg.sources[0].mapping.default_prefix = s("pre/fix2");
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
    }

    #[fbinit::test]
    fn test_verify_conflicting_prefixes_megarepo_path(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let mut cfg = get_good_cfg();
        cfg.sources[0].mapping.default_prefix = s(MEGAREPO_SPECIAL_PATH);
        assert!(verify_paths(&ctx, &cfg).is_err());

        Ok(())
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
