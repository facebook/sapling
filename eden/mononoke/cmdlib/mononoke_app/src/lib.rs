/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod app;
pub mod args;
mod builder;
mod extension;
pub mod fb303;

pub use app::MononokeApp;
pub use builder::MononokeAppBuilder;
pub use extension::ArgExtension;

#[doc(hidden)]
pub mod macro_export {
    pub use anyhow;
    pub use clap;
    pub use heck;
}

/// Define subcommands for a Mononoke App
///
/// This macro is a convenience macro for defining a Mononoke App with
/// subcommands, where each subcommand gets its own module.
///
/// To use this:
///
/// * Create `commands.rs` with a call to this macro.  List each module
///   that defines a subcommand, which should be placed in a `commands`
///   directory.
///
/// ```
/// use mononoke_app::subcommands;
///
/// subcommands! {
///    mod first_command;
///    mod second_command;
/// }
/// ```
///
/// * Each command should provide two things at the root of its module:
///
///   * A `CommandArgs` type which should derive `clap::Parser` and represents
///     the options available to this subcommand.
///
///   * A function to run the command, with the signature:
///     `pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()>`.
///
/// * This macro gathers these together and provides a `subcommands` function
///   and a `dispatch` function.
///
///   * Call `commands::subcommands()` to construct the subcommands and pass
///     them to `MononokeAppBuilder::build_with_subcommands` for your app.
///
///   * When executing the app, call `commands::dispatch(app)` to run the
///     selected subcommand.
#[macro_export]
macro_rules! subcommands {
    ( $( mod $command:ident; )* ) => {
        $( mod $command; )*

        /// Add args for all commands.
        pub(crate) fn subcommands<'help>() -> Vec<$crate::macro_export::clap::App<'help>> {
            use $crate::macro_export::clap::IntoApp;
            use $crate::macro_export::heck::KebabCase;
            vec![
                $(
                    $command::CommandArgs::into_app()
                        .name(stringify!($command).to_kebab_case()),
                )*
            ]
        }

        /// Dispatch a command invocation.
        pub(crate) async fn dispatch(app: $crate::MononokeApp) -> $crate::macro_export::anyhow::Result<()> {
            use $crate::macro_export::clap::FromArgMatches;
            use $crate::macro_export::heck::SnakeCase;
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
