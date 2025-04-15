/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl remove
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::Subcommand;

mod operations;
mod types;
mod utils;

use types::Messenger;
use types::PathType;
use types::RemoveContext;

#[derive(Parser, Debug)]
#[clap(name = "remove", about = "Remove an EdenFS checkout")]
pub struct RemoveCmd {
    #[clap(
        multiple_values = true,
        help = "The EdenFS checkout(s) to remove.",
        value_name = "PATH"
    )]
    paths: Vec<String>,

    #[clap(
            short = 'y',
            long = "yes",
            visible_aliases = &["--no-prompt"],
            help = "Do not prompt for confirmation before removing the checkouts."
        )]
    skip_prompt: bool,

    // Do not print to stdout. This is independent with '--no-prompt'
    #[clap(short = 'q', long = "quiet", hide = true)]
    suppress_output: bool,

    // Answer no for any prompt.
    // This is only used in testing the path when a user does not confirm upon the prompt
    // I have to this because dialoguer::Confirm does not accept input from non-terminal
    // https://github.com/console-rs/dialoguer/issues/170
    //
    // When provided with "-y": undefined!
    #[clap(short = 'n', long = "answer-no", hide = true)]
    no: bool,

    #[clap(long, hide = true)]
    preserve_mount_point: bool,

    #[clap(long = "--no-force", hide = true)]
    no_force: bool,
}

#[async_trait]
impl Subcommand for RemoveCmd {
    async fn run(&self) -> Result<ExitCode> {
        if self.skip_prompt && self.no {
            return Err(anyhow!(
                "Both '-y' and '-n' are provided. This is not supported.\nExiting."
            ));
        }

        let mut type_paths_map: HashMap<PathType, Vec<&str>> = HashMap::new();
        let mut remove_contexts: Vec<RemoveContext> = Vec::new();

        let messenger = Arc::new(Messenger::new_stdio(
            self.skip_prompt,
            self.suppress_output,
            self.no,
        ));

        for path in &self.paths {
            let (canonicalized_path, path_type) = operations::classify_path(path).await?;

            let context = RemoveContext::new(
                path.clone(),
                canonicalized_path,
                path_type,
                self.preserve_mount_point,
                self.no_force,
                messenger.clone(),
            );
            remove_contexts.push(context);

            let paths = match path_type {
                PathType::InactiveEdenMount => {
                    // InactiveEdenMount and ActiveEdenMount share the same prompt
                    // so we need to combine them together
                    type_paths_map
                        .entry(PathType::ActiveEdenMount)
                        .or_insert(Vec::new())
                }
                _ => type_paths_map.entry(path_type).or_insert(Vec::new()),
            };
            paths.push(path);
        }

        // show the prompt with aggregated information
        if !self.skip_prompt {
            let mut prompts: Vec<String> = Vec::new();
            for t in [
                PathType::ActiveEdenMount,
                PathType::RegularFile,
                PathType::Unknown,
            ]
            .into_iter()
            {
                if type_paths_map.contains_key(&t) {
                    let paths = type_paths_map.get(&t).unwrap();
                    let prompt = t.get_prompt(paths.to_vec());
                    prompts.push(prompt);
                }
            }

            if !messenger.prompt_user(prompts.join("\n"))? {
                return Err(anyhow!(
                    "User did not confirm the removal. Stopping. Nothing removed!"
                ));
            }
        }

        for context in remove_contexts {
            context.path_type.remove(&context).await?;
        }

        messenger.success(format!(
            "\nSuccessfully removed:\n{}",
            self.paths.join("\n")
        ));
        Ok(0)
    }
}
