// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate futures;
#[macro_use]
extern crate slog;

extern crate async_unit;
extern crate slog_glog_fmt;

extern crate blobrepo_utils;
extern crate mercurial_types;

extern crate branch_even;
extern crate branch_uneven;
extern crate branch_wide;
extern crate linear;
extern crate merge_even;
extern crate merge_uneven;
extern crate unshared_merge_even;
extern crate unshared_merge_uneven;

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

                use blobrepo_utils::{BonsaiVerify, BonsaiVerifyResult};
                use mercurial_types::HgChangesetId;

                use $repo;

                #[test]
                fn test() {
                    async_unit::tokio_unit_test(|| {
                        let drain = glog_drain().filter_level(Level::Debug).fuse();
                        let logger = Logger::root(drain, o![]);

                        let repo = $repo::getrepo(Some(logger.clone()));
                        let heads = repo.get_heads()
                            .collect()
                            .wait()
                            .expect("getting all heads should work");
                        let heads = heads.into_iter().map(HgChangesetId::new);

                        let verify = BonsaiVerify {
                            logger,
                            repo,
                            follow_limit: 1024,
                            ignores: HashSet::new(),
                            debug_bonsai_diff: false,
                        };
                        let verify_stream = verify.verify(heads);
                        let results = verify_stream
                            .collect()
                            .wait()
                            .expect("verifying should work");
                        let diffs = results.into_iter().filter_map(|(res, meta)| {
                            match res {
                                BonsaiVerifyResult::Invalid(difference) => {
                                    let changes = difference
                                        .changes()
                                        .collect()
                                        .wait()
                                        .expect("collecting diff for inconsistency should work");
                                    Some((meta.changeset_id, changes))
                                }
                                _ => None,
                            }
                        });

                        let mut failed = false;
                        let mut desc = Vec::new();
                        for (changeset_id, changes) in diffs {
                            failed = true;
                            desc.push(format!(
                                "*** Inconsistent roundtrip for {}",
                                changeset_id,
                            ));
                            for changed_entry in changes {
                                desc.push(format!("  - Changed entry: {:?}", changed_entry));
                            }
                            desc.push("".to_string());
                        }
                        let desc = desc.join("\n");
                        if failed {
                            panic!("Inconsistencies detected, roundtrip test failed\n\n{}", desc);
                        }
                    })
                }
            }
        }
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
