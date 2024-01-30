/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeSet;

use anyhow::bail;
use clidispatch::abort_if;
use clidispatch::errors;
use clidispatch::OptionalRepo;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configloader::Config;
use configmodel::ConfigExt;
use configmodel::ValueSource;
use formatter::formatter::FormatOptions;
use formatter::formatter::Formattable;
use formatter::formatter::ListFormatter;
use minibytes::Text;
use serde::ser::Serialize;
use serde::ser::SerializeStruct;
use serde::ser::Serializer;

use super::get_formatter;
use super::ConfigSet;
use super::Result;
use crate::commands::FormatterOpts;

define_flags! {
    pub struct ConfigOpts {
        /// edit config, implying --user if no other flags set (DEPRECATED)
        #[short('e')]
        edit: bool,

        /// edit user config, opening in editor if no args given
        #[short('u')]
        user: bool,

        /// edit repository config, opening in editor if no args given
        #[short('l')]
        local: bool,

        /// edit system config, opening in editor if no args given (DEPRECATED)
        #[short('g')]
        global: bool,

        /// edit system config, opening in editor if no args given
        #[short('s')]
        system: bool,

        /// delete specified config items
        #[short('d')]
        delete: bool,

        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<ConfigOpts>, repo: &mut OptionalRepo) -> Result<u8> {
    let config = repo.config();
    let force_rust = config
        .get_or_default::<Vec<String>>("commands", "force-rust")?
        .contains(&"config".to_owned());
    let use_rust = force_rust || config.get_or_default("config", "use-rust")?;

    if !use_rust {
        bail!(errors::FallbackToPython(
            "config.use-rust not set to True".to_owned()
        ));
    }

    if ctx.opts.edit
        || ctx.opts.local
        || ctx.opts.global
        || ctx.opts.user
        || ctx.opts.system
        || ctx.opts.delete
    {
        bail!(errors::FallbackToPython(
            "config edit options not supported in Rust".to_owned()
        ));
    }

    let config = repo.config();
    let mut formatter = get_formatter(
        config,
        short_name(),
        &ctx.opts.formatter_opts.template,
        ctx.global_opts(),
        Box::new(ctx.io().output()),
    )?;

    ctx.maybe_start_pager(repo.config())?;

    formatter.begin_list()?;
    let exit_code = show_configs(ctx, config, formatter.as_mut())?;
    formatter.end_list()?;

    Ok(exit_code)
}

struct ConfigItem<'a> {
    source: String,
    all_sources: Cow<'a, [ValueSource]>,
    section: &'a str,
    key: &'a str,
    value: Option<String>,
    single_item: bool,
    builtin: bool,
}

impl<'a> Serialize for ConfigItem<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut item = serializer.serialize_struct("ConfigItem", 3)?;
        let name = format!("{}.{}", self.section, self.key);
        item.serialize_field("name", name.as_str())?;
        item.serialize_field("source", &self.source)?;

        item.serialize_field("value", &self.value)?;

        item.end()
    }
}

impl<'a> Formattable for ConfigItem<'a> {
    fn format_plain(
        &self,
        options: &FormatOptions,
        writer: &mut dyn formatter::StyleWrite,
    ) -> std::result::Result<(), anyhow::Error> {
        let value: &str = match &self.value {
            Some(value) => value.as_ref(),
            None if options.debug => "<%unset>",
            _ => return Ok(()),
        };

        let source_section = if options.debug {
            format!("{}: ", self.source)
        } else {
            "".to_string()
        };
        let kv_section = if !self.single_item {
            format!("{}.{}=", self.section, self.key)
        } else {
            "".to_string()
        };
        write!(
            writer,
            "{}{}{}\n",
            source_section,
            kv_section,
            value.replace('\n', "\\n")
        )?;

        if options.debug && options.verbose {
            for s in self.all_sources.iter().rev().skip(1) {
                let value = match &s.value {
                    None => Text::from_static("<%unset>"),
                    Some(value) => value.clone(),
                };
                write!(
                    writer,
                    "  {}: {kv_section}{}\n",
                    source_to_display_string(s),
                    value.replace('\n', "\\n"),
                )?;
            }
        }

        Ok(())
    }
}

fn get_config_item<'a>(
    config: &'a ConfigSet,
    section: &'a str,
    key: &'a str,
    single_item: bool,
    debug: bool,
) -> Option<ConfigItem<'a>> {
    let all_sources = config.get_sources(section, key);
    let config_value_source = match all_sources.last() {
        None => {
            return None;
        }
        Some(last) => last,
    };

    let value = config_value_source.value();

    // Don't expose %unset unless --debug was specified.
    if value.is_none() && !debug {
        return None;
    }

    Some(ConfigItem {
        source: source_to_display_string(config_value_source),
        section,
        key,
        value: value.as_ref().map(|v| v.to_string()),
        single_item,
        builtin: config_value_source.source().starts_with("builtin:"),
        all_sources,
    })
}

fn source_to_display_string(source: &ValueSource) -> String {
    source
        .location()
        .and_then(|(location, range)| {
            source.file_content().map(|file| {
                let line = 1 + file
                    .slice(0..range.start)
                    .chars()
                    .filter(|ch| *ch == '\n')
                    .count();
                if !location.as_os_str().is_empty() {
                    format!("{}:{}", location.display(), line)
                } else {
                    format!("{}:{}", source.source(), line)
                }
            })
        })
        .unwrap_or_else(|| source.source().to_string())
}

fn show_configs(
    ctx: ReqCtx<ConfigOpts>,
    config: &ConfigSet,
    formatter: &mut dyn ListFormatter,
) -> Result<u8> {
    let verbose = ctx.global_opts().verbose;
    let debug = ctx.global_opts().debug;

    let requested_items: Vec<_> = ctx
        .opts
        .args
        .iter()
        .filter(|a| a.contains('.'))
        .cloned()
        .collect();
    let requested_sections: BTreeSet<_> = ctx
        .opts
        .args
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

        if let Some(item) = get_config_item(config, parts[0], parts[1], true, debug) {
            formatter.format_item(&item)?;
            return Ok(0);
        }
        // Config is expected to return an empty string if anything goes wrong
        return Ok(1);
    }

    let config_sections: BTreeSet<Text> = config.sections().as_ref().iter().cloned().collect();
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
            if let Some(item) = get_config_item(config, section, &key, false, debug) {
                if empty_selection && item.builtin && !verbose {
                    continue;
                }
                formatter.format_item(&item)?;
            }
        }
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "config|showconfig|debugconfig|conf|confi"
}

pub fn doc() -> &'static str {
    r#"show config settings

    With no arguments, print names and values of all config items.

    With one argument of the form ``section.name``, print just the value
    of that config item.

    With multiple arguments, print names and values of all config
    items with matching section names.

    With ``--user``, edit the user-level config file. With ``--system``,
    edit the system-wide config file. With ``--local``, edit the
    repository-level config file. If there are no arguments, spawn
    an editor to edit the config file. If there are arguments in
    ``section.name=value`` or ``section.name value`` format, the appropriate
    config file will be updated directly without spawning an editor.

    With ``--delete``, the specified config items are deleted from the config
    file.

    With ``--debug``, the source (filename and line number) is printed
    for each config item.

    See :prog:`help config` for more information about config files.

    Returns 0 on success, 1 if NAME does not exist.

    "#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... [NAME]...")
}

fn short_name() -> &'static str {
    "config"
}
