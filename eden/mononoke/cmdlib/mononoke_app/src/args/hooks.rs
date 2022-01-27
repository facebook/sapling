/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Result};
use clap::Args;
use slog::{warn, Logger};
use std::collections::{HashMap, HashSet};

/// Command line arguments for tweaking hooks
#[derive(Args, Debug)]
pub struct HooksArgs {
    /// Disable a hook. Pass this argument multiple times to disable multiple hooks.
    #[clap(long = "disable-hook")]
    disabled_hooks: Vec<String>,
}

impl HooksArgs {
    pub fn process_disabled_with_repo_prefix(
        &self,
        logger: &Logger,
    ) -> Result<HashMap<String, HashSet<String>>> {
        let mut res = HashMap::new();
        for repohook in &self.disabled_hooks {
            let repohook: Vec<_> = repohook.splitn(2, ':').collect();
            let repo = repohook.get(0);
            let hook = repohook.get(1);

            let (repo, hook) =
                repo.and_then(|repo| hook.map(|hook| (repo, hook)))
                    .ok_or(format_err!(
                        "invalid format of disabled hook, should be 'REPONAME:HOOKNAME'"
                    ))?;
            res.entry(repo.to_string())
                .or_insert_with(HashSet::new)
                .insert(hook.to_string());
        }
        if !res.is_empty() {
            warn!(logger, "The following Hooks were disabled: {:?}", res);
        }
        Ok(res)
    }

    pub fn process_disabled_no_repo_prefix(&self, logger: &Logger) -> HashSet<String> {
        if !self.disabled_hooks.is_empty() {
            warn!(
                logger,
                "The following Hooks were disabled: {:?}", self.disabled_hooks
            );
        }

        self.disabled_hooks.iter().cloned().collect()
    }
}
