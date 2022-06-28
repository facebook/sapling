/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Error;
use clap::App;
use clap::Arg;
use clap::ArgMatches;
use fbinit::FacebookInit;
use futures_util::stream::TryStreamExt;

use crate::connection::Connection;
use crate::render::RenderStream;

const ARG_JSON: &str = "JSON";
const ENV_WRITES_ENABLED: &str = "SCSC_WRITES_ENABLED";

macro_rules! commands {
    ( $( mod $command:ident $(if $env:ident)? ; )* ) => {
        $( mod $command; )*

        /// Add args for all commands.
        pub(crate) fn add_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
            let mut app = app.arg(
                Arg::with_name(ARG_JSON)
                    .long("json")
                    .global(true)
                    .help("Output as a stream of JSON objects"),
            );
            $(
                $( if std::env::var($env).is_ok() )?
                {
                    app = app.subcommand($command::make_subcommand());
                }
            )*
            app
        }

        /// Dispatch a command invocation.
        pub(crate) async fn dispatch(fb: FacebookInit, matches: ArgMatches<'_>) -> Result<(), Error> {
            let connection = Connection::from_args(fb, &matches)?;
            let target = if matches.is_present(ARG_JSON) {
                OutputTarget::Json
            } else if atty::is(atty::Stream::Stdout) {
                OutputTarget::Tty
            } else {
                OutputTarget::Pipe
            };
            match matches.subcommand() {
                $( ($command::NAME, Some(m)) => output(target, m, $command::run(m, connection).await?).await, )*
                (name, _) => unreachable!("command {} not recognized", name),
            }
        }
    }
}

commands! {
    mod cat;
    mod common_base;
    mod create_bookmark if ENV_WRITES_ENABLED;
    mod delete_bookmark if ENV_WRITES_ENABLED;
    mod diff;
    mod export;
    mod info;
    mod is_ancestor;
    mod land_stack if ENV_WRITES_ENABLED;
    mod list_bookmarks;
    mod log;
    mod lookup;
    mod ls;
    mod move_bookmark if ENV_WRITES_ENABLED;
    mod repos;
    mod run_hooks;
    mod blame;
    mod xrepo_lookup;
    mod lookup_pushrebase_history;
}

#[derive(Copy, Clone, Debug)]
enum OutputTarget {
    Tty,
    Pipe,
    Json,
}

/// Render the output for a command invocation.
async fn output(
    target: OutputTarget,
    matches: &ArgMatches<'_>,
    render: RenderStream,
) -> Result<(), Error> {
    render
        .try_for_each(move |output| async move {
            let mut stdout = std::io::stdout();
            match target {
                OutputTarget::Tty => {
                    output.render_tty(matches, &mut stdout)?;
                }
                OutputTarget::Pipe => {
                    output.render(matches, &mut stdout)?;
                }
                OutputTarget::Json => {
                    output.render_json(matches, &mut stdout)?;
                    writeln!(&mut stdout)?;
                }
            }
            Ok(())
        })
        .await
}
