// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Example logs created by this module:
//! Results of running hooks
//!   5d34ec2d 1
//!     OK file hooks:
//!       - block_cross_repo_commits: tested on 6 files
//!     no changeset hooks to run
//!     0 of 1 file hooks failed
//!     ACCEPTED

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use itertools::Itertools;
use mercurial::changeset::RevlogChangeset;
use mercurial_types::HgChangesetId;
use slog::Logger;

use hooks::{ChangesetHookExecutionID, FileHookExecutionID, HookExecution};

fn into_nested_group_map<K1, K2, V>(
    iterable: impl IntoIterator<Item = (K1, (K2, V))>,
) -> HashMap<K1, BTreeMap<K2, Vec<V>>>
where
    K1: Hash + Eq,
    K2: Hash + Eq + Ord,
{
    iterable
        .into_iter()
        .into_group_map()
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                v.into_iter()
                    .into_group_map()
                    .into_iter()
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect()
}

fn fix_indentation(s: &str) -> String {
    s.replace('\n', "\n        ")
}

fn log_changeset_hook_results(
    logger: &Logger,
    results: &HashMap<bool, Vec<(&str, &HookExecution)>>,
) {
    if let Some(results) = results.get(&true) {
        let mut results = results.clone();
        results.sort_by(|(a, _), (b, _)| a.cmp(b));
        info!(logger, "    OK changeset hooks:");
        for (ref name, _) in results {
            info!(logger, "      - {}", name);
        }
    }
    if let Some(results) = results.get(&false) {
        let mut results = results.clone();
        results.sort_by(|(a, _), (b, _)| a.cmp(b));
        info!(logger, "    FAILED changeset hooks:");
        for (ref name, ref res) in results {
            let info = match res {
                HookExecution::Rejected(info) => info,
                bad => panic!(
                    "Impossible, this is part of a different group by: {:?}",
                    bad
                ),
            };
            info!(logger, "      - {}: {}", name, info.description);
            if info.long_description != "" {
                info!(
                    logger,
                    "        {}",
                    fix_indentation(&info.long_description)
                );
            }
        }
    }
}

fn log_file_hook_results(
    logger: &Logger,
    results: &HashMap<bool, BTreeMap<&str, Vec<(&str, &HookExecution)>>>,
) {
    if let Some(results) = results.get(&true) {
        info!(logger, "    OK file hooks:");
        for (name, per_file_ex) in results {
            info!(
                logger,
                "      - {}: tested on {} files",
                name,
                per_file_ex.len()
            );
        }
    }
    if let Some(results) = results.get(&false) {
        info!(logger, "    FAILED file hooks:");
        for (name, per_file_ex) in results {
            let mut per_file_ex = per_file_ex.clone();
            per_file_ex.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (ref path, ref res) in per_file_ex {
                let info = match res {
                    HookExecution::Rejected(info) => info,
                    bad => panic!(
                        "Impossible, this is part of a different group by: {:?}",
                        bad
                    ),
                };
                info!(logger, "      - {} on {}: {}", name, path, info.description);
                if info.long_description != "" {
                    info!(
                        logger,
                        "        {}",
                        fix_indentation(&info.long_description)
                    );
                }
            }
        }
    }
}

pub fn log_results_of_hooks(
    logger: &Logger,
    hg_cs_id: HgChangesetId,
    revlog_cs: &RevlogChangeset,
    cs_hook_results: &Vec<(ChangesetHookExecutionID, HookExecution)>,
    file_hook_results: &Vec<(FileHookExecutionID, HookExecution)>,
) {
    let cs_hook_results: HashMap<bool, Vec<(&str, &HookExecution)>> = cs_hook_results
        .iter()
        .map(|(ref ex_id, ref ex_res)| (ex_res.is_accepted(), (ex_id.hook_name.as_str(), ex_res)))
        .into_group_map();

    let file_hook_results: HashMap<bool, BTreeMap<&str, Vec<(&str, &HookExecution)>>> =
        into_nested_group_map(file_hook_results.iter().map(|(ref ex_id, ref ex_res)| {
            (
                ex_res.is_accepted(),
                (ex_id.hook_name.as_str(), (ex_id.file.path.as_str(), ex_res)),
            )
        }));

    let first_line_comment = {
        String::from_utf8_lossy(revlog_cs.comments())
            .split('\n')
            .next()
            .unwrap_or("")
            .to_owned()
    };

    info!(logger, "  {:.8} {}", hg_cs_id, first_line_comment);
    log_changeset_hook_results(logger, &cs_hook_results);
    log_file_hook_results(logger, &file_hook_results);

    let cs_accepted = {
        let accept_cnt = cs_hook_results.get(&true).map(|v| v.len()).unwrap_or(0);
        let reject_cnt = cs_hook_results.get(&false).map(|v| v.len()).unwrap_or(0);
        let total_cnt = accept_cnt + reject_cnt;
        if total_cnt == 0 {
            info!(logger, "    no changeset hooks to run");
        } else {
            info!(
                logger,
                "    {} of {} changeset hooks failed", reject_cnt, total_cnt
            );
        }
        reject_cnt == 0
    };

    let file_accepted = {
        let accept_cnt = file_hook_results.get(&true).map(|v| v.len()).unwrap_or(0);
        let reject_cnt = file_hook_results.get(&false).map(|v| v.len()).unwrap_or(0);
        let total_cnt = accept_cnt + reject_cnt;
        if total_cnt == 0 {
            info!(logger, "    no file hooks to run");
        } else {
            info!(
                logger,
                "    {} of {} file hooks failed", reject_cnt, total_cnt
            );
        }
        reject_cnt == 0
    };

    if cs_accepted && file_accepted {
        info!(logger, "    ACCEPTED");
    } else {
        info!(logger, "    REJECTED");
    }
}
