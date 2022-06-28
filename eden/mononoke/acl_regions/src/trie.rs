/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::AclRegionRule;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub struct RegionIndex(pub usize);

/// A trie that stores all paths from all regions for efficient rule matching
/// by path.
#[derive(Default)]
pub struct PrefixTrieWithRules {
    children: HashMap<MPathElement, PrefixTrieWithRules>,
    associated_rules: Vec<(RegionIndex, Arc<AclRegionRule>)>,
}

impl PrefixTrieWithRules {
    pub fn new() -> PrefixTrieWithRules {
        PrefixTrieWithRules {
            children: HashMap::new(),
            associated_rules: vec![],
        }
    }

    /// Add all paths of all regions of the rule and store the rule at the end
    /// of each path.
    pub fn add_rule(&mut self, rule: Arc<AclRegionRule>) {
        for (region_index, region) in rule.regions.iter().enumerate() {
            for path in &region.path_prefixes {
                self.add_rule_on_path(
                    path.iter().flatten(),
                    RegionIndex(region_index),
                    rule.clone(),
                );
            }
        }
    }

    /// Add a given path and store the associated rule at the end.
    fn add_rule_on_path<'a>(
        &mut self,
        path: impl IntoIterator<Item = &'a MPathElement>,
        region_index: RegionIndex,
        rule: Arc<AclRegionRule>,
    ) {
        let mut iter = path.into_iter();
        match iter.next() {
            Some(element) => self
                .children
                .entry(element.clone())
                .or_default()
                .add_rule_on_path(iter, region_index, rule),
            None => self.associated_rules.push((region_index, rule)),
        }
    }

    /// Traverse a given path and collect all rules alongside it. Return them
    /// deduplicated by (rule name, matched region index)
    pub fn associated_rules(
        &self,
        path: Option<&MPath>,
    ) -> HashMap<(String, RegionIndex), Arc<AclRegionRule>> {
        self.associated_rules_inner(path.into_iter().flatten())
            .into_iter()
            .map(|(region_index, rule)| ((rule.name.clone(), region_index), rule))
            .collect()
    }

    fn associated_rules_inner<'a>(
        &self,
        path: impl IntoIterator<Item = &'a MPathElement>,
    ) -> Vec<(RegionIndex, Arc<AclRegionRule>)> {
        let mut iter = path.into_iter();
        let mut rules = match iter.next() {
            None => vec![],
            Some(element) => match self.children.get(element) {
                Some(child) => child.associated_rules_inner(iter),
                None => vec![],
            },
        };
        rules.reserve(self.associated_rules.len());
        rules.extend(self.associated_rules.iter().cloned());
        rules
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use metaconfig_types::AclRegion;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    fn path(raw: &str) -> Option<MPath> {
        MPath::new_opt(raw).unwrap()
    }

    struct TestData {
        path: Option<MPath>,
        expected_regions: HashSet<(String, RegionIndex)>,
    }

    impl TestData {
        fn new(raw_path: &str, raw_regions: &[(&str, usize)]) -> Self {
            let expected_regions = raw_regions
                .iter()
                .map(|(name, index)| (name.to_string(), RegionIndex(*index)))
                .collect::<HashSet<_>>();
            let path = path(raw_path);
            Self {
                path,
                expected_regions,
            }
        }

        fn verify(&self, trie: &PrefixTrieWithRules) {
            let rules = trie.associated_rules(self.path.as_ref());
            assert_eq!(
                rules.into_iter().map(|(k, _)| k).collect::<HashSet<_>>(),
                self.expected_regions
            );
        }
    }

    #[fbinit::test]
    fn test_prefix_trie() {
        let mut trie = PrefixTrieWithRules::new();
        trie.add_rule(Arc::new(AclRegionRule {
            name: "rule1".to_string(),
            regions: vec![
                AclRegion {
                    roots: vec![],
                    heads: vec![],
                    path_prefixes: vec![path("a"), path("c/d")],
                },
                AclRegion {
                    roots: vec![],
                    heads: vec![],
                    path_prefixes: vec![path("c"), path("c/d")],
                },
            ],
            hipster_acl: "acl1".to_string(),
        }));
        trie.add_rule(Arc::new(AclRegionRule {
            name: "rule2".to_string(),
            regions: vec![AclRegion {
                roots: vec![],
                heads: vec![],
                path_prefixes: vec![path("a/b"), path("b")],
            }],
            hipster_acl: "acl2".to_string(),
        }));
        trie.add_rule(Arc::new(AclRegionRule {
            name: "rule3".to_string(),
            regions: vec![AclRegion {
                roots: vec![],
                heads: vec![],
                path_prefixes: vec![path("")],
            }],
            hipster_acl: "acl3".to_string(),
        }));

        let test_data = vec![
            TestData::new("a", &[("rule1", 0), ("rule3", 0)]),
            TestData::new("aa", &[("rule3", 0)]),
            TestData::new("a/b/c", &[("rule1", 0), ("rule2", 0), ("rule3", 0)]),
            TestData::new("a/bb", &[("rule1", 0), ("rule3", 0)]),
            TestData::new("b/a", &[("rule2", 0), ("rule3", 0)]),
            TestData::new("c/d", &[("rule1", 0), ("rule1", 1), ("rule3", 0)]),
        ];

        for (index, data) in test_data.iter().enumerate() {
            eprintln!("Verifying test data #{}", index);
            data.verify(&trie);
        }
    }
}
