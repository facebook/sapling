/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use acl_regions::AclRegions;
use acl_regions::AclRegionsRef;
use acl_regions::AssociatedRulesResult;
use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use metaconfig_types::AclRegion;
use metaconfig_types::AclRegionConfig;
use metaconfig_types::AclRegionRule;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use pretty_assertions::assert_eq;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use std::collections::HashSet;
use test_repo_factory::TestRepoFactory;
use tests_utils::drawdag::create_from_dag;

#[facet::container]
#[derive(Clone)]
struct Repo {
    #[facet]
    acl_regions: dyn AclRegions,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    changesets: dyn Changesets,

    #[facet]
    filestore_config: FilestoreConfig,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    changeset_fetcher: dyn ChangesetFetcher,
}

fn path(p: &str) -> Option<MPath> {
    MPath::new_opt(p).unwrap()
}

struct TestData {
    cs_id: ChangesetId,
    path: Option<MPath>,
    expected_names: HashSet<String>,
}

impl TestData {
    fn new(cs_id: ChangesetId, raw_path: &str, raw_names: &[&str]) -> Self {
        let expected_names = raw_names
            .iter()
            .map(ToString::to_string)
            .collect::<HashSet<String>>();
        let path = path(raw_path);
        Self {
            cs_id,
            path,
            expected_names,
        }
    }

    async fn verify(&self, ctx: &CoreContext, acl_regions: &dyn AclRegions) -> Result<()> {
        let res = acl_regions
            .associated_rules(ctx, self.cs_id, self.path.as_ref())
            .await?;
        let rules = match res {
            AssociatedRulesResult::AclRegionsDisabled => {
                panic!("AclRegions should work in test, but they're disabled")
            }
            AssociatedRulesResult::Rules(rules) => rules,
        };
        assert_eq!(
            rules
                .into_iter()
                .map(|descriptor| descriptor.name)
                .collect::<HashSet<String>>(),
            self.expected_names
        );
        Ok(())
    }
}

#[fbinit::test]
async fn test_acl_regions(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mut factory = TestRepoFactory::new(fb)?;
    let repo: Repo = factory.build()?;
    let dag = r#"
          I
         /
    A-B-C-D-G-H
       \   /
        E-F
    "#;
    let mapping = create_from_dag(&ctx, &repo, dag).await?;
    let a = mapping["A"];
    let b = mapping["B"];
    let c = mapping["C"];
    let d = mapping["D"];
    let e = mapping["E"];
    let f = mapping["F"];
    let g = mapping["G"];
    let h = mapping["H"];
    let i = mapping["I"];

    let rule1 = AclRegionRule {
        name: "rule1".to_string(),
        regions: vec![AclRegion {
            roots: vec![],
            heads: vec![b],
            path_prefixes: vec![path("")],
        }],
        hipster_acl: "acl1".to_string(),
    };

    let rule2 = AclRegionRule {
        name: "rule2".to_string(),
        regions: vec![
            AclRegion {
                roots: vec![a],
                heads: vec![],
                path_prefixes: vec![path("a")],
            },
            AclRegion {
                roots: vec![h],
                heads: vec![],
                path_prefixes: vec![path("aa")],
            },
        ],
        hipster_acl: "acl2".to_string(),
    };

    let rule3 = AclRegionRule {
        name: "rule3".to_string(),
        regions: vec![
            AclRegion {
                roots: vec![b],
                heads: vec![e, i],
                path_prefixes: vec![path("b")],
            },
            AclRegion {
                roots: vec![e],
                heads: vec![g],
                path_prefixes: vec![path("b/c")],
            },
        ],
        hipster_acl: "acl3".to_string(),
    };

    // We're re-creating the repo with a correct config after we got hashes
    // from the mapping.
    let repo: Repo = factory
        .with_config_override(|config| {
            config.acl_region_config = Some(AclRegionConfig {
                allow_rules: vec![rule1, rule2, rule3],
            });
        })
        .build()?;

    let same_mapping = create_from_dag(&ctx, &repo, dag).await?;
    assert_eq!(mapping, same_mapping);

    let acl_regions = repo.acl_regions();

    let test_data = vec![
        TestData::new(a, "a", &["rule1", "rule2"]),
        TestData::new(a, "b", &["rule1"]),
        TestData::new(b, "", &[]),
        TestData::new(b, "b", &["rule3"]),
        TestData::new(c, "a", &["rule2"]),
        TestData::new(c, "b", &["rule3"]),
        TestData::new(d, "b", &["rule3"]),
        TestData::new(e, "b", &[]),
        TestData::new(e, "b/c", &["rule3"]),
        TestData::new(f, "b/c", &["rule3"]),
        TestData::new(g, "aa", &[]),
        TestData::new(g, "b", &[]),
        TestData::new(g, "b/c", &[]),
        TestData::new(h, "b", &[]),
        TestData::new(h, "aa", &["rule2"]),
        TestData::new(i, "b", &[]),
    ];

    for (index, data) in test_data.iter().enumerate() {
        eprintln!("Verifying test data #{}", index);
        data.verify(&ctx, acl_regions).await?;
    }

    Ok(())
}
