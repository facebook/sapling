// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::command::{CommandDefinition, CommandFunc, CommandTable};
use crate::errors;
use crate::global_flags::HgGlobalOpts;
use crate::io::IO;
use crate::repo::OptionalRepo;
use bytes::Bytes;
use cliparser::alias::{expand_aliases, expand_prefix};
use cliparser::parser::{ParseError, ParseOptions, ParseOutput, StructFlags};
use configparser::config::ConfigSet;
use configparser::hg::parse_list;
use failure::Fallible;
use std::{collections::BTreeMap, env, path::Path};

/// Similar to `env::args()`. But does not panic.
pub fn args() -> Fallible<Vec<String>> {
    env::args_os()
        .map(|os| {
            os.into_string()
                .map_err(|_| errors::NonUTF8Arguments.into())
        })
        .collect()
}

/// Apply config override flags.
fn override_config<P>(
    config: &mut ConfigSet,
    config_paths: &[P],
    config_overrides: &[String],
) -> Fallible<()>
where
    P: AsRef<Path>,
{
    let mut errors = Vec::new();

    for config_path in config_paths {
        errors.extend(config.load_path(config_path, &"--configfile".into()));
    }

    for config_override in config_overrides {
        let equals_pos = config_override
            .find("=")
            .ok_or_else(|| errors::MalformedConfigOption(config_override.to_string()))?;
        let section_name_pair = &config_override[..equals_pos];
        let value = &config_override[equals_pos + 1..];

        let dot_pos = section_name_pair
            .find(".")
            .ok_or_else(|| errors::MalformedConfigOption(config_override.to_string()))?;
        let section = &section_name_pair[..dot_pos];
        let name = &section_name_pair[dot_pos + 1..];

        config.set(section, name, Some(&Bytes::from(value)), &"--config".into());
    }

    Ok(())
}

fn find_command_name(has_command: impl Fn(&str) -> bool, args: Vec<String>) -> Option<String> {
    let mut command_name = None;
    for arg in args {
        if command_name.is_none() {
            if has_command(&arg) {
                command_name = Some(arg);
            } else {
                return None;
            }
        } else {
            // To check for subcommands we continue iterating to see if a longer valid command
            // is able to be created.
            //
            // $ hg cloud sync -> will become Some("cloud") then attempt "cloud sync".
            let orig = command_name.unwrap();
            let curr = orig.clone() + &arg;
            if has_command(&curr) {
                command_name = Some(curr.to_string())
            } else {
                return Some(orig);
            }
        }
    }
    command_name
}

fn last_chance_to_abort(opts: &HgGlobalOpts) -> Fallible<()> {
    if opts.profile {
        return Err(errors::Abort("--profile does not support Rust commands (yet)".into()).into());
    }

    if opts.help {
        return Err(errors::FallbackToPython.into());
    }

    Ok(())
}

fn early_parse(args: &Vec<String>) -> Result<ParseOutput, ParseError> {
    ParseOptions::new()
        .ignore_prefix(true)
        .early_parse(true)
        .flags(HgGlobalOpts::flags())
        .flag_alias("repo", "repository")
        .parse_args(args)
}

fn command_map<'a>(
    definitions: impl IntoIterator<Item = &'a CommandDefinition>,
    cfg: &ConfigSet,
) -> BTreeMap<String, isize> {
    let mut command_map = BTreeMap::new();
    let mut i = 1;

    for command in definitions {
        let name = command.name();
        let is_debug = name.starts_with("debug");
        command_map.insert(name.to_string(), if is_debug { -i } else { i });
        i = i + 1;
    }
    // adding aliases into the command map is what Python does, so copying this behavior
    // allows alias expansion to not behave differently for Rust or Python.
    for name in cfg.keys("alias") {
        if let Ok(name) = String::from_utf8(name.to_vec()) {
            if name.contains(":") {
                // Not a real command.
                continue;
            }
            let is_debug = name.starts_with("debug");
            command_map.insert(name, if is_debug { -i } else { i });
            i = i + 1;
        }
    }

    // Names from `commands.name` config.
    // This is a fast (but inaccurate) way to know Python command names.
    let config_commands = parse_list(cfg.get("commands", "names").unwrap_or_default());
    for b_name in config_commands {
        if let Ok(name) = String::from_utf8(b_name.to_vec()) {
            let is_debug = name.starts_with("debug");
            for name in name.split("|") {
                command_map.insert(name.to_string(), if is_debug { -i } else { i });
            }
            i = i + 1;
        }
    }

    command_map
}

fn parse(definition: &CommandDefinition, args: &Vec<String>) -> Result<ParseOutput, ParseError> {
    let flags = definition
        .flags()
        .into_iter()
        .chain(HgGlobalOpts::flags().into_iter())
        .collect();
    ParseOptions::new()
        .error_on_unknown_opts(true)
        .flags(flags)
        .flag_alias("repo", "repository")
        .parse_args(args)
}

pub fn dispatch(command_table: &CommandTable, mut args: Vec<String>, io: &mut IO) -> Fallible<u8> {
    let early_result = early_parse(&args)?;
    let global_opts: HgGlobalOpts = early_result.clone().into();

    if !global_opts.cwd.is_empty() {
        env::set_current_dir(global_opts.cwd)?;
    }

    // Load repo and configuration.
    let mut optional_repo = OptionalRepo::from_repository_path_and_cwd(
        &global_opts.repository,
        &env::current_dir()?,
        configparser::hg::load()?,
    )?;
    override_config(
        optional_repo.config_mut(),
        &global_opts.configfile,
        &global_opts.config,
    )?;
    let config = optional_repo.config();

    // Prepare alias handling.
    let alias_lookup = |name: &str| {
        // [alias] can have "<name>:doc" entries that are not commands. Skip them.
        if name.contains(":") {
            return None;
        }

        match (config.get("alias", name), config.get("defaults", name)) {
            (None, None) => None,
            (Some(v), None) => String::from_utf8(v.to_vec()).ok(),
            (None, Some(v)) => String::from_utf8(v.to_vec())
                .ok()
                .map(|v| format!("{} {}", name, v)),
            (Some(a), Some(d)) => {
                if let (Ok(a), Ok(d)) =
                    (String::from_utf8(a.to_vec()), String::from_utf8(d.to_vec()))
                {
                    // XXX: This makes defaults override alias if there are conflicted
                    // flags. The desired behavior is to make alias override defaults.
                    // However, [defaults] is deprecated and is likely only used
                    // by tests. So this might be fine.
                    Some(format!("{} {}", a, d))
                } else {
                    None
                }
            }
        }
    };

    let command_map = command_map(command_table.values(), &config);

    let early_args = early_result.args();
    let first_arg = early_args
        .get(0)
        .ok_or_else(|| errors::UnknownCommand(String::new()))?;

    let first_arg_index = early_result.first_arg_index();

    // This should hold true since `first_arg` is not empty (tested above).
    // Therefore positional args is non-empty and first_arg_index should be
    // an index in args.
    debug_assert!(first_arg_index < args.len());
    debug_assert_eq!(&args[first_arg_index], first_arg);

    let command_name = expand_prefix(&command_map, first_arg)?;
    args[first_arg_index] = command_name.clone();

    let (expanded, _first_arg_indexd) = expand_aliases(alias_lookup, &args[first_arg_index..])?;

    let mut new_args = Vec::new();

    new_args.extend_from_slice(&args[..first_arg_index]);
    new_args.extend_from_slice(&expanded[..]);

    let command_name = find_command_name(|name| command_table.contains_key(name), expanded)
        .ok_or_else(|| errors::UnknownCommand(command_name))?;

    let full_args = new_args;

    let def = &command_table[&command_name];
    let parsed = parse(&def, &full_args)?;

    let global_opts: HgGlobalOpts = parsed.clone().into();
    last_chance_to_abort(&global_opts)?;

    let handler = def.func();

    match handler {
        CommandFunc::Repo(f) => {
            match optional_repo {
                OptionalRepo::Some(repo) => f(parsed, io, repo),
                OptionalRepo::None(_config) => {
                    // FIXME: Try to "infer repo" here.
                    Err(errors::RepoRequired(
                        env::current_dir()
                            .ok()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )
                    .into())
                }
            }
        }
        CommandFunc::OptionalRepo(f) => match optional_repo {
            OptionalRepo::Some(repo) => f(parsed, io, Some(repo)),
            OptionalRepo::None(_config) => f(parsed, io, None),
        },
        CommandFunc::NoRepo(f) => f(parsed, io),
    }
}
