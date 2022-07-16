/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::path::Path;
use std::sync::atomic::Ordering::SeqCst;

use anyhow::Error;
use cliparser::alias::expand_aliases;
use cliparser::alias::find_command_name;
use cliparser::parser::ParseError;
use cliparser::parser::ParseOptions;
use cliparser::parser::ParseOutput;
use cliparser::parser::StructFlags;
use configparser::config::ConfigSet;
use repo::repo::OptionalRepo;
use repo::repo::Repo;

use crate::command::CommandDefinition;
use crate::command::CommandFunc;
use crate::command::CommandTable;
use crate::errors;
use crate::global_flags::HgGlobalOpts;
use crate::io::IO;

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
    config.set_overrides(config_overrides)?;

    Ok(())
}

fn add_global_flag_derived_configs(repo: &mut OptionalRepo, global_opts: HgGlobalOpts) {
    if let OptionalRepo::Some(_) = repo {
        if global_opts.hidden {
            let config = repo.config_mut();
            config.set("visibility", "all-heads", Some("true"), &"--hidden".into());
        }
    }

    let config = repo.config_mut();
    if global_opts.trace || global_opts.traceback {
        config.set("ui", "traceback", Some("on"), &"--traceback".into());
    }
    if global_opts.profile {
        config.set("profiling", "enabled", Some("true"), &"--profile".into());
    }
    if !global_opts.color.is_empty() {
        config.set(
            "ui",
            "color",
            Some(global_opts.color.as_str()),
            &"--color".into(),
        );
    }
    if global_opts.verbose || global_opts.debug || global_opts.quiet {
        config.set(
            "ui",
            "verbose",
            Some(global_opts.verbose.to_string().as_str()),
            &"--verbose".into(),
        );
        config.set(
            "ui",
            "debug",
            Some(global_opts.debug.to_string().as_str()),
            &"--debug".into(),
        );
        config.set(
            "ui",
            "quiet",
            Some(global_opts.quiet.to_string().as_str()),
            &"--quiet".into(),
        );
    }
    if global_opts.noninteractive {
        config.set("ui", "interactive", Some("off"), &"-y".into());
    }
}

fn last_chance_to_abort(opts: &HgGlobalOpts) -> Result<()> {
    if opts.profile {
        return Err(errors::Abort("--profile does not support Rust commands (yet)".into()).into());
    }

    if opts.help {
        return Err(errors::FallbackToPython("help").into());
    }

    Ok(())
}

fn early_parse(args: &[String]) -> Result<ParseOutput, ParseError> {
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
                configparser::convert::ByteCount::from(1u64 << 12)
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
    if cfg!(unix) {
        let chmod_file = config.get_or("permissions", "chmod-file", || -1)?;
        if chmod_file >= 0 {
            indexedlog::utils::CHMOD_FILE.store(chmod_file, SeqCst);
        }

        let chmod_dir = config.get_or("permissions", "chmod-dir", || -1)?;
        if chmod_dir >= 0 {
            indexedlog::utils::CHMOD_DIR.store(chmod_dir, SeqCst);
        }

        let use_symlink_atomic_write: bool =
            config.get_or_default("format", "use-symlink-atomic-write")?;
        indexedlog::utils::SYMLINK_ATOMIC_WRITE.store(use_symlink_atomic_write, SeqCst);
    }

    let fsync: bool = config.get_or_default("storage", "indexedlog-fsync")?;
    indexedlog::utils::set_global_fsync(fsync);

    Ok(())
}

pub fn parse_global_opts(args: &[String]) -> Result<HgGlobalOpts> {
    let early_result = early_parse(args)?;
    early_result.try_into()
}

pub struct Dispatcher {
    args: Vec<String>,
    early_result: ParseOutput,
    global_opts: HgGlobalOpts,
    optional_repo: OptionalRepo,
}

fn version_args() -> Vec<String> {
    vec!["version".to_string()]
}

impl Dispatcher {
    /// Load configs. Prepare to run a command.
    pub fn from_args(mut args: Vec<String>) -> Result<Self> {
        if args.get(0).map(|s| s.as_ref()) == Some("--version") {
            args = version_args();
        }

        let mut early_result = early_parse(&args)?;
        let global_opts: HgGlobalOpts = early_result.clone().try_into()?;
        if global_opts.version {
            args = version_args();
            early_result = early_parse(&args)?;
        }

        let cwd = if global_opts.cwd.is_empty() {
            Path::new(".")
        } else {
            Path::new(&global_opts.cwd)
        };
        let cwd = util::path::absolute(cwd)?;

        // Load repo and configuration.
        match OptionalRepo::from_repository_path_and_cwd(&global_opts.repository, &cwd) {
            Ok(mut optional_repo) => {
                override_config(
                    optional_repo.config_mut(),
                    &global_opts.configfile,
                    &global_opts.config,
                )?;

                Ok(Self {
                    args,
                    early_result,
                    global_opts,
                    optional_repo,
                })
            }
            Err(err) => {
                // If we failed to load the repo, make one last ditch effort to load a repo-less config.
                // This might allow us to run the network doctor even if this repo's dynamic config is not loadable.
                if let Ok(mut config) = configparser::hg::load::<String, String>(None, None) {
                    override_config(&mut config, &global_opts.configfile, &global_opts.config)?;
                    Err(errors::triage_error(&config, err))
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Get a reference to the parsed config.
    pub fn config(&self) -> &ConfigSet {
        self.optional_repo.config()
    }

    /// Get a reference to the global options.
    pub fn global_opts(&self) -> &HgGlobalOpts {
        &self.global_opts
    }

    pub fn repo(&self) -> Option<&Repo> {
        match &self.optional_repo {
            OptionalRepo::Some(repo) => Some(repo),
            _ => None,
        }
    }

    /// Replace OptionalRepo::Some with OptionalRepo::None(config)
    /// where config is not influenced by the current repo.
    pub fn convert_to_repoless_config(&mut self) -> Result<()> {
        if matches!(self.optional_repo, OptionalRepo::Some(_)) {
            self.optional_repo = OptionalRepo::None(self.load_repoless_config()?)
        }

        Ok(())
    }

    fn load_repoless_config(&self) -> Result<ConfigSet> {
        let mut config = configparser::hg::load::<String, String>(None, None)?;
        override_config(
            &mut config,
            &self.global_opts.configfile,
            &self.global_opts.config,
        )?;
        Ok(config)
    }

    fn prepare_command<'a>(
        &self,
        command_table: &'a CommandTable,
        io: &IO,
    ) -> Result<(&'a CommandDefinition, ParseOutput)> {
        let args = &self.args;
        let early_result = &self.early_result;
        let config = self.optional_repo.config();

        if !self.global_opts.cwd.is_empty() {
            env::set_current_dir(&self.global_opts.cwd)?;
        }

        initialize_indexedlog(&config)?;

        // Prepare alias handling.
        let alias_lookup = |name: &str| {
            // [alias] can have "<name>:doc" entries that are not commands. Skip them.
            if name.contains(":") {
                return None;
            }

            match (config.get("alias", name), config.get("defaults", name)) {
                (None, None) => None,
                (Some(v), None) => Some(v.to_string()),
                (None, Some(v)) => Some(format!("{} {}", name, v.as_ref())),
                (Some(a), Some(d)) => {
                    // XXX: This makes defaults override alias if there are conflicted
                    // flags. The desired behavior is to make alias override defaults.
                    // However, [defaults] is deprecated and is likely only used
                    // by tests. So this might be fine.
                    Some(format!("{} {}", a.as_ref(), d.as_ref()))
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
        tracing::info!(
            name = "log:command-row",
            command = AsRef::<str>::as_ref(&command_name)
        );

        let mut new_args = Vec::with_capacity(args.len());
        new_args.extend_from_slice(&args[..first_arg_index]);
        new_args.push(command_name.clone());
        new_args.extend_from_slice(&expanded[command_arg_len..]);

        let def = command_table.get(&command_name).unwrap();
        let parsed = parse(def, &new_args)?;

        let global_opts: HgGlobalOpts = parsed.clone().try_into()?;
        last_chance_to_abort(&global_opts)?;

        initialize_blackbox(&self.optional_repo)?;

        if global_opts.pager == "always" {
            io.start_pager(self.optional_repo.config())?;
        }

        Ok((def, parsed))
    }

    /// Run a command. Return exit code if the command completes.
    pub fn run_command<'a>(
        &mut self,
        command_table: &'a CommandTable,
        io: &IO,
    ) -> (Option<&'a CommandDefinition>, Result<u8>) {
        let (handler, parsed) = match self.prepare_command(command_table, io) {
            Ok((name, args)) => (name, args),
            Err(e) => return (None, Err(e)),
        };

        let res = || -> Result<u8> {
            add_global_flag_derived_configs(&mut self.optional_repo, parsed.clone().try_into()?);
            match handler.func() {
                CommandFunc::Repo(f) => {
                    match self.optional_repo {
                        OptionalRepo::Some(ref mut repo) => f(parsed, io, repo),
                        OptionalRepo::None(_) => {
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
                CommandFunc::OptionalRepo(f) => f(parsed, io, &mut self.optional_repo),
                CommandFunc::NoRepo(f) => {
                    self.convert_to_repoless_config()?;
                    f(parsed, io, self.optional_repo.config_mut())
                }
                CommandFunc::NoRepoGlobalOpts(f) => {
                    self.convert_to_repoless_config()?;
                    f(parsed, io, self.optional_repo.config_mut())
                }
                CommandFunc::OptionalRepoGlobalOpts(f) => f(parsed, io, &mut self.optional_repo),
            }
        }();

        (Some(handler), res)
    }
}
