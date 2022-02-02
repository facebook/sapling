/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use clap::{App, FromArgMatches, IntoApp};
use heck::{KebabCase, SnakeCase};
use mononoke_app::MononokeApp;

macro_rules! commands {
    ( $( mod $command:ident; )* ) => {
        $( mod $command; )*

        /// Add args for all commands.
        pub(crate) fn subcommands<'help>() -> Vec<App<'help>> {
            vec![
                $(
                    $command::CommandArgs::into_app()
                        .name(stringify!($command).to_kebab_case()),
                )*
            ]
        }

        /// Dispatch a command invocation.
        pub(crate) async fn dispatch(app: MononokeApp) -> Result<()> {
            if let Some((name, matches)) = app.subcommand() {
                match name.to_snake_case().as_str() {
                    $(
                        stringify!($command) => {
                            let args = $command::CommandArgs::from_arg_matches(matches)?;
                            $command::run(app, args).await
                        }
                    )*
                    _ => Err(anyhow!("unrecognised subcommand: {}", name)),
                }
            } else {
                Err(anyhow!("no subcommand specified"))
            }
        }
    }
}

commands! {
    mod blobstore;
    mod blobstore_unlink;
    mod fetch;
    mod list_repos;
    mod repo_info;
}
