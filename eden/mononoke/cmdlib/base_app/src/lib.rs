/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgMatches;

#[doc(hidden)]
pub mod macro_export {
    pub use anyhow;
    pub use clap;
    pub use heck;
    pub use static_assertions::assert_impl_all;
}

pub trait BaseApp {
    /// Returns the selected subcommand of the app (if this app
    /// has subcommands).
    fn subcommand(&self) -> Option<(&str, &ArgMatches)>;
}

/// Lower level version of mononoke_app::subcommands that allows changing the app
#[macro_export]
macro_rules! subcommands {
     ( type App = $app:ty; $( mod $command:ident $(if $env:literal)? ; )* ) => {
         $( mod $command; )*

         $crate::macro_export::assert_impl_all!($app: $crate::BaseApp);

         /// Add args for all commands.
         pub(crate) fn subcommands<'help>() -> Vec<$crate::macro_export::clap::Command<'help>> {
             use $crate::macro_export::clap::IntoApp;
             use $crate::macro_export::heck::KebabCase;
             let mut apps = vec![];
            $(
                $( if std::env::var($env).is_ok() )?
                {
                    apps.push($command::CommandArgs::command()
                        .name(stringify!($command).to_kebab_case()));
                }
            )*

            apps
         }

         /// Dispatch a command invocation.
         pub(crate) async fn dispatch(app: $app) -> $crate::macro_export::anyhow::Result<()> {
             use $crate::macro_export::clap::FromArgMatches;
             use $crate::macro_export::heck::SnakeCase;
             use $crate::BaseApp;
             if let Some((name, matches)) = app.subcommand() {
                 match name.to_snake_case().as_str() {
                     $(
                         stringify!($command) => {
                             let args = $command::CommandArgs::from_arg_matches(matches)?;
                             $command::run(app, args).await
                         }
                     )*
                     _ => Err($crate::macro_export::anyhow::anyhow!("unrecognised subcommand: {}", name)),
                 }
             } else {
                 Err($crate::macro_export::anyhow::anyhow!("no subcommand specified"))
             }
         }
     }
 }
