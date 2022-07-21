/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::AppExtension;
use anyhow::Context;
use anyhow::Result;
use clap::Args;
use environment::MononokeEnvironment;
use slog::warn;
use std::collections::HashMap;
use std::collections::HashSet;

/// Command line arguments for tweaking hooks
#[derive(Args, Debug)]
pub struct HooksArgs {
    /// Disable a hook. Pass this argument multiple times to disable multiple hooks.
    #[clap(long = "disable-hook")]
    disabled_hooks: Vec<String>,
}

pub struct HooksAppExtension;

impl AppExtension for HooksAppExtension {
    type Args = HooksArgs;

    fn environment_hook(&self, args: &Self::Args, env: &mut MononokeEnvironment) -> Result<()> {
        let mut res = HashMap::new();
        for repohook in &args.disabled_hooks {
            let repohook: Vec<_> = repohook.splitn(2, ':').collect();
            let repo = repohook.get(0);
            let hook = repohook.get(1);

            let (repo, hook) = repo
                .and_then(|repo| hook.map(|hook| (repo, hook)))
                .context("invalid format of disabled hook, should be 'REPONAME:HOOKNAME'")?;
            res.entry(repo.to_string())
                .or_insert_with(HashSet::new)
                .insert(hook.to_string());
        }

        if !res.is_empty() {
            warn!(env.logger, "The following Hooks were disabled: {:?}", res);
        }

        env.disabled_hooks = res;

        Ok(())
    }
}
