// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use fixtures::*;

// An extra level of nesting is required to avoid clashes between crate and module names.
mod test {
    macro_rules! test_verify {
        ($repo:ident) => {
            mod $repo {
                use std::collections::HashSet;

                use futures::{Future, Stream};
                use slog::{Drain, Level, Logger};

                use async_unit;
                use slog_glog_fmt::default_drain as glog_drain;

                use blobrepo_utils::{BonsaiMFVerify, BonsaiMFVerifyResult};
                use context::CoreContext;

                use crate::$repo;

                #[test]
                fn test() {
                    async_unit::tokio_unit_test(|| {
                        let ctx = CoreContext::test_mock();
                        let drain = glog_drain().filter_level(Level::Debug).fuse();
                        let logger = Logger::root(drain, slog::o![]);

                        let repo = $repo::getrepo();
                        let heads = repo.get_heads_maybe_stale(ctx.clone()).collect();

                        let verify = BonsaiMFVerify {
                            ctx: ctx.clone(),
                            logger,
                            repo,
                            follow_limit: 1024,
                            ignores: HashSet::new(),
                            broken_merges_before: None,
                            debug_bonsai_diff: false,
                        };

                        let results = heads
                            .map_err(|err| panic!("cannot get the heads {}", err))
                            .and_then(|heads| verify.verify(heads).collect());
                        tokio::spawn(
                            results
                                .and_then(move |results| {
                                    let diffs = results.into_iter().filter_map(
                                        move |(res, meta)| match res {
                                            BonsaiMFVerifyResult::Invalid(difference) => {
                                                let cs_id = meta.changeset_id;
                                                Some(
                                                    difference
                                                        .changes(ctx.clone())
                                                        .collect()
                                                        .map(move |changes| (cs_id, changes)),
                                                )
                                            }
                                            _ => None,
                                        },
                                    );

                                    futures::future::join_all(diffs)
                                })
                                .map(|diffs| {
                                    let mut failed = false;
                                    let mut desc = Vec::new();
                                    for (changeset_id, changes) in diffs {
                                        failed = true;
                                        desc.push(format!(
                                            "*** Inconsistent roundtrip for {}",
                                            changeset_id,
                                        ));
                                        for changed_entry in changes {
                                            desc.push(format!(
                                                "  - Changed entry: {:?}",
                                                changed_entry
                                            ));
                                        }
                                        desc.push("".to_string());
                                    }
                                    let desc = desc.join("\n");
                                    if failed {
                                        panic!(
                                            "Inconsistencies detected, roundtrip test failed\n\n{}",
                                            desc
                                        );
                                    }
                                })
                                .map_err(|err| {
                                    panic!("verify error {}", err);
                                }),
                        );
                    })
                }
            }
        };
    }

    test_verify!(branch_even);
    test_verify!(branch_uneven);
    test_verify!(branch_wide);
    test_verify!(linear);
    test_verify!(merge_even);
    test_verify!(merge_uneven);
    test_verify!(unshared_merge_even);
    test_verify!(unshared_merge_uneven);
}
