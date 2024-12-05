/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::Any;
use std::sync::Arc;

use anyhow::Result;
use clap::ArgMatches;
use clap::Args;
use clap::Command;
use clap::FromArgMatches;
use environment::MononokeEnvironment;

/// Trait implemented by things that need to extend the app building process,
/// including adding additional arguments and modifying the environment before
/// it is used to start Mononoke.
pub trait AppExtension: Send + Sync + 'static {
    /// Argument type to extend Mononoke arguments with.
    type Args: clap::Args + Send + Sync + 'static;

    /// Obtain default values for these arguments.
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Hook executed after creating the environment before initializing Mononoke.
    fn environment_hook(&self, _args: &Self::Args, _env: &mut MononokeEnvironment) -> Result<()> {
        Ok(())
    }
}

// Internal trait to hide the concrete extension type.
pub(crate) trait BoxedAppExtension: Send + Sync + 'static {
    fn augment_args(&self, app: Command) -> Command;
    fn arg_defaults(&self) -> Vec<(&'static str, String)>;
    fn parse_args(&self, args: &ArgMatches) -> Result<Box<dyn BoxedAppExtensionArgs>>;
}

// Box to store an app extension.
#[derive(Clone)]
pub(crate) struct AppExtensionBox<Ext: AppExtension> {
    ext: Arc<Ext>,
}

impl<Ext: AppExtension> AppExtensionBox<Ext> {
    pub(crate) fn new(ext: Ext) -> Box<dyn BoxedAppExtension> {
        Box::new(AppExtensionBox { ext: Arc::new(ext) })
    }
}

impl<Ext: AppExtension> BoxedAppExtension for AppExtensionBox<Ext> {
    fn augment_args(&self, command: Command) -> Command {
        Ext::Args::augment_args_for_update(command)
    }

    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        self.ext.arg_defaults()
    }

    fn parse_args(&self, args: &ArgMatches) -> Result<Box<dyn BoxedAppExtensionArgs>> {
        let args = Ext::Args::from_arg_matches(args)?;
        Ok(Box::new(AppExtensionArgsBox {
            ext: self.ext.clone(),
            args,
        }))
    }
}

pub(crate) trait Downcast: Any {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> Downcast for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Internal trait to hide the concrete extension args type.
pub(crate) trait BoxedAppExtensionArgs: Downcast + Send + Sync + 'static {
    fn environment_hook(&self, env: &mut MononokeEnvironment) -> Result<()>;
}

// Box to store an app extension and its parsed args.
pub(crate) struct AppExtensionArgsBox<Ext: AppExtension> {
    ext: Arc<Ext>,
    args: Ext::Args,
}

impl<Ext: AppExtension> AppExtensionArgsBox<Ext> {
    pub(crate) fn args(&self) -> &Ext::Args {
        &self.args
    }
}

impl<Ext: AppExtension> BoxedAppExtensionArgs for AppExtensionArgsBox<Ext> {
    fn environment_hook(&self, env: &mut MononokeEnvironment) -> Result<()> {
        self.ext.environment_hook(&self.args, env)
    }
}
