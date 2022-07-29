/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::bail;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::errors;
use clidispatch::global_flags::HgGlobalOpts;
use clidispatch::io::IsTty;
use clidispatch::OptionalRepo;
use cliparser::define_flags;
use configparser::Config;
use minibytes::Text;

use super::ConfigSet;
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
    config_opts: ConfigOpts,
    global_opts: HgGlobalOpts,
    io: &IO,
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

    if config_opts.edit
        || config_opts.local
        || config_opts.global
        || !config_opts.formatter_opts.template.is_empty()
        || global_opts.debug
    {
        bail!(errors::FallbackToPython(short_name()));
    }

    let config = repo.config();

    if io.output().is_tty() {
        io.start_pager(config)?;
    }

    show_configs(config_opts.args, io, config)
}

fn show_configs(requested_configs: Vec<String>, io: &IO, config: &ConfigSet) -> Result<u8> {
    let requested_items: Vec<_> = requested_configs
        .iter()
        .filter(|a| a.contains('.'))
        .cloned()
        .collect();
    let requested_sections: BTreeSet<_> = requested_configs
        .into_iter()
        .filter_map(|a| {
            if !a.contains('.') {
                Some(Text::from(a))
            } else {
                None
            }
        })
        .collect();

    abort_if!(requested_items.len() > 1, "only one config item permitted");
    abort_if!(
        !requested_items.is_empty() && !requested_sections.is_empty(),
        "combining sections and items not permitted"
    );

    if requested_items.len() == 1 {
        let item = &requested_items[0];
        let parts: Vec<_> = item.splitn(2, '.').collect();
        if let Some(value) = config.get_nonempty(parts[0], parts[1]) {
            io.write(format!("{}\n", value))?;
            return Ok(0);
        }
        // Config is expected to return an empty string if anything goes wrong
        return Ok(1);
    }

    let config_sections: BTreeSet<_> = BTreeSet::from_iter(config.sections());
    let empty_selection = requested_sections.is_empty();
    let selected_sections: Box<dyn Iterator<Item = &Text>> = if empty_selection {
        Box::new(config_sections.iter())
    } else {
        Box::new(requested_sections.intersection(&config_sections))
    };
    let mut selected_sections = selected_sections.peekable();

    if selected_sections.peek().is_none() {
        return Ok(1);
    }

    for section in selected_sections {
        let mut keys = config.keys(section);
        keys.sort();
        for key in keys {
            if empty_selection
                && config
                    .get_sources(&section, &key)
                    .iter()
                    .any(|source| source.source().to_string().as_str() == "builtin.rc")
            {
                continue;
            }
            if let Some(value) = config.get(&section, &key) {
                io.write(format!(
                    "{}.{}={}\n",
                    section,
                    key,
                    value.replace('\n', "\\n")
                ))?;
            }
        }
    }

    Ok(0)
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
