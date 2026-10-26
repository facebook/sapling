/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use mononoke_app::MononokeApp;

/// Print the messages that hook implementations declare, as JSON
///
/// For each implementation, lists its messages with their descriptions,
/// default text and parameters. A repo replaces a message's text through the
/// `messages` map of the hook's config.
#[derive(Parser)]
pub struct CommandArgs {
    /// Only print the messages of this hook implementation.
    #[clap(long)]
    implementation: Option<String>,
}

pub async fn run(_app: MononokeApp, args: CommandArgs) -> Result<()> {
    let messages: Vec<_> = hooks::all_hook_messages()
        .into_iter()
        .filter(|messages| {
            args.implementation
                .as_deref()
                .is_none_or(|implementation| messages.implementation == implementation)
        })
        .collect();
    if messages.is_empty()
        && let Some(implementation) = &args.implementation
    {
        bail!("No hook implementation named '{implementation}' declares messages");
    }
    println!("{}", serde_json::to_string_pretty(&messages)?);
    Ok(())
}
