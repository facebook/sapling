/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::command::{CommandDefinition, CommandFunc, CommandTable};
use crate::errors;
use crate::global_flags::HgGlobalOpts;
use crate::io::IO;
use crate::repo::OptionalRepo;
use anyhow::Error;
use bytes::Bytes;
use cliparser::alias::{expand_aliases, find_command_name};
use cliparser::parser::{ParseError, ParseOptions, ParseOutput, StructFlags};
use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use std::convert::TryInto;
use std::{env, path::Path};

type Result<T, E = Error> = std::result::Result<T, E>;

/// Similar to `env::args()`. But does not panic.
pub fn args() -> Result<Vec<String>> {
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
) -> Result<()>
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

fn last_chance_to_abort(opts: &HgGlobalOpts) -> Result<()> {
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

fn initialize_blackbox(optional_repo: &OptionalRepo) -> Result<()> {
    if let OptionalRepo::Some(repo) = optional_repo {
        let config = repo.config();
        let max_size = config
            .get_or("blackbox", "maxsize", || {
                configparser::hg::ByteCount::from(1u64 << 12)
            })?
            .value();
        let max_files = config.get_or("blackbox", "maxfiles", || 3)?;
        let path = repo.shared_path().join(".hg/blackbox/v1");
        if let Ok(blackbox) = ::blackbox::BlackboxOptions::new()
            .max_bytes_per_log(max_size)
            .max_log_count(max_files as u8)
            .open(path)
        {
            ::blackbox::init(blackbox);
        }
    }
    Ok(())
}

fn initialize_indexedlog(config: &ConfigSet) -> Result<()> {
    #[cfg(unix)]
    {
        use std::sync::atomic::Ordering::SeqCst;
        let chown_group = config.get_or("permissions", "chown-group", || String::new())?;
        if !chown_group.is_empty() {
            // Try to find the group from /etc/group
            if let Ok(groups) = std::fs::read_to_string("/etc/group") {
                let prefix = format!("{}:", chown_group);
                for line in groups.lines() {
                    // group_name:password:GID:user_list
                    if line.starts_with(&prefix) {
                        if let Some(gid_str) = line.split(":").nth(2) {
                            if let Ok(gid) = gid_str.parse::<i64>() {
                                indexedlog::utils::CHOWN_GID.store(gid, SeqCst);
                            }
                        }
                    }
                }
            }
        }

        let chmod_file = config.get_or("permissions", "chmod-file", || -1)?;
        if chmod_file >= 0 {
            indexedlog::utils::CHMOD_FILE.store(chmod_file, SeqCst);
        }

        let chmod_dir = config.get_or("permissions", "chmod-dir", || -1)?;
        if chmod_dir >= 0 {
            indexedlog::utils::CHMOD_DIR.store(chmod_dir, SeqCst);
        }
    }

    Ok(())
}

pub fn dispatch(command_table: &CommandTable, args: Vec<String>, io: &mut IO) -> Result<u8> {
    let early_result = early_parse(&args)?;
    let global_opts: HgGlobalOpts = early_result.clone().try_into()?;

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

    initialize_indexedlog(&config)?;

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

    // The difference between args, expanded and new_args is:
    // - args are unchanged arguments provided by the user.
    //   args can have global flags before command name.
    //   for example, ["hg", "--traceback", "log", "-Gvr", "master"]
    //                                      ^^^^^ first_arg_index, "log" is "command_name"
    // - expanded: includes alias expansion result
    //   no global flags before command name.
    //   for example, with alias "log = log -f", ["log", "-Gvr", "master"]
    //   will be expanded to ["log", "-f", "-Gvr", "master"].
    // - new_args: final args to parse, like expanded with global flags.
    //   ["hg", "--traceback", "log", "-f", "-Gvr", "master"].

    let command_name = first_arg.to_string();
    let (expanded, _first_arg_index) = expand_aliases(alias_lookup, &args[first_arg_index..])?;
    let (command_name, command_arg_len) =
        find_command_name(|name| command_table.get(name).is_some(), &expanded)
            .ok_or_else(|| errors::UnknownCommand(command_name))?;

    let mut new_args = Vec::with_capacity(args.len());
    new_args.extend_from_slice(&args[..first_arg_index]);
    new_args.push(command_name.clone());
    new_args.extend_from_slice(&expanded[command_arg_len..]);

    let full_args = new_args;

    let def = command_table.get(&command_name).unwrap();
    let parsed = parse(&def, &full_args)?;

    let global_opts: HgGlobalOpts = parsed.clone().try_into()?;
    last_chance_to_abort(&global_opts)?;

    initialize_blackbox(&optional_repo)?;

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
