/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use clidispatch::abort;
use clidispatch::errors;
use clidispatch::global_flags::HgGlobalOpts;
use cliparser::define_flags;
use repo::repo::OptionalRepo;

use super::Result;
use super::IO;

use crate::commands::FormatterOpts;

define_flags! {
    pub struct ConfigOpts {
        /// show untrusted configuration options
        #[short('u')]
        untrusted: bool,

        /// edit user config
        #[short('e')]
        edit: bool,

        /// edit repository config
        #[short('l')]
        local: bool,

        /// edit global config
        #[short('g')]
        global: bool,

        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(
    _config_opts: ConfigOpts,
    _global_opts: HgGlobalOpts,
    _io: &IO,
    repo: &mut OptionalRepo,
) -> Result<u8> {
    let config = repo.config();
    let force_rust = config
        .get_or_default::<Vec<String>>("commands", "force-rust")?
        .contains(&"config".to_owned());
    let use_rust = force_rust || config.get_or_default("config", "use-rust")?;

    if !use_rust {
        bail!(errors::FallbackToPython(short_name()));
    }

    abort!("Not implemented")
}

pub fn name() -> &'static str {
    "config|showconfig|debugconfig|conf|confi"
}

pub fn doc() -> &'static str {
    r#"show config settings

    With no arguments, print names and values of all config items.

    With one argument of the form section.name, print just the value
    of that config item.

    With multiple arguments, print names and values of all config
    items with matching section names.

    With --edit, start an editor on the user-level config file. With
    --global, edit the system-wide config file. With --local, edit the
    repository-level config file.

    With --debug, the source (filename and line number) is printed
    for each config item.

    See :hg:`help config` for more information about config files.

    Returns 0 on success, 1 if NAME does not exist.

    "#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[-u] [NAME]...")
}

fn short_name() -> &'static str {
    "config"
}
