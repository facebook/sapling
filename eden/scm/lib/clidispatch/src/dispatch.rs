/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::env;
use std::path::Path;
use std::sync::Arc;

use anyhow::Error;
use cliparser::alias::expand_aliases;
use cliparser::alias::find_command_name;
use cliparser::parser::ParseError;
use cliparser::parser::ParseOptions;
use cliparser::parser::ParseOutput;
use cliparser::parser::StructFlags;
use cliparser::parser::Value;
use configloader::config::ConfigSet;
use configloader::hg::RepoInfo;
use configloader::hg::set_pinned;
use configmodel::Config;
use configmodel::ConfigExt;
use hgtime::HgTime;
use repo::repo::Repo;

use crate::OptionalRepo;
use crate::abort_if;
use crate::command::CommandDefinition;
use crate::command::CommandFunc;
use crate::command::CommandTable;
use crate::errors;
use crate::errors::UnknownCommand;
use crate::global_flags::HgGlobalOpts;
use crate::io::IO;
use crate::util::pinned_configs;

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

fn last_chance_to_abort(early: &HgGlobalOpts, full: &HgGlobalOpts) -> Result<()> {
    if full.help {
        return Err(errors::UnknownCommand("--help".to_string()).into());
    }

    // These are security sensitive options, so perform extra checks.
    //
    // "early" was parsed disallowing arbitrary prefix matching (e.g.
    // "--configfi" won't expand to "--configfile"), so simply comparing the
    // early and full args can detect abbreviations.
    //
    // These comparisons also check for the sensitive options being included in
    // command aliases since aliases have not been expanded for the "early"
    // parse.
    abort_if!(
        early.config != full.config,
        "option --config may not be abbreviated, used in aliases, or used as a value for another option",
    );

    abort_if!(
        early.configfile != full.configfile,
        "option --configfile may not be abbreviated or used in aliases",
    );

    abort_if!(
        early.cwd != full.cwd,
        "option --cwd may not be abbreviated or used in aliases",
    );

    abort_if!(
        early.repository != full.repository,
        "option -R must appear alone, and --repository may not be abbreviated or used in aliases",
    );

    abort_if!(
        early.debugger != full.debugger,
        "option --debugger may not be abbreviated or used in aliases",
    );

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

fn parse(definition: &CommandDefinition, args: &[String]) -> Result<ParseOutput, ParseError> {
    let flags = definition
        .flags()
        .into_iter()
        .chain(HgGlobalOpts::flags())
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
                configloader::convert::ByteCount::from(1u64 << 12)
            })?
            .value();
        let max_files = config.get_or("blackbox", "maxfiles", || 3)?;
        let path = repo.shared_dot_hg_path().join("blackbox/v1");
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

fn initialize_hgtime(config: &dyn Config) -> Result<()> {
    let mut should_clear = true;
    if let Some(now_str) = config.get("devel", "default-date") {
        let now_str = now_str.trim();
        if !now_str.is_empty() {
            if let Some(now) = HgTime::parse(now_str) {
                tracing::info!(?now, "set 'now' for testing");
                now.set_as_now_for_testing();
                should_clear = false;
            }
        }
    }
    if should_clear {
        tracing::debug!("unset 'now' for testing");
        HgTime::clear_now_for_testing();
    }
    Ok(())
}

fn initialize_libraries(config: &dyn Config) -> Result<()> {
    indexedlog::config::configure(config)?;
    gitcompat::GlobalGit::set_default_config(config);
    Ok(())
}

pub fn parse_global_opts(args: &[String]) -> Result<HgGlobalOpts> {
    let early_result = early_parse(args)?;
    early_result.try_into()
}

pub struct Dispatcher {
    orig_args: Vec<String>,
    args: Vec<String>,
    early_result: ParseOutput,
    early_global_opts: HgGlobalOpts,
    optional_repo: OptionalRepo,
}

fn version_args(binary_path: &str) -> Vec<String> {
    vec![binary_path.to_string(), "version".to_string()]
}

impl Dispatcher {
    /// Load configs. Prepare to run a command.
    pub fn from_args(mut args: Vec<String>) -> Result<Self> {
        let orig_args = args.clone();

        if args.get(1).map(|s| s.as_ref()) == Some("--version") {
            args[1] = "version".to_string();
        }

        let mut early_result = early_parse(&args[1..])?;
        let global_opts: HgGlobalOpts = early_result.clone().try_into()?;
        if global_opts.version {
            args = version_args(&args[0]);
            if global_opts.quiet {
                args.push("--quiet".to_string());
            }
            if global_opts.verbose {
                args.push("--verbose".to_string());
            }
            early_result = early_parse(&args[1..])?;
        }

        let cwd = if global_opts.cwd.is_empty() {
            Path::new(".")
        } else {
            Path::new(&global_opts.cwd)
        };
        let cwd = util::path::absolute(cwd)?;

        // Load repo and configuration.
        match OptionalRepo::from_global_opts(&global_opts, cwd) {
            Ok(optional_repo) => Ok(Self {
                orig_args,
                args,
                early_result,
                early_global_opts: global_opts,
                optional_repo,
            }),
            Err(err) => {
                // If we failed to load the repo, make one last ditch effort to load a repo-less config.
                // This might allow us to run the network doctor even if this repo's dynamic config is not loadable.
                if let Ok(config) =
                    configloader::hg::load(RepoInfo::NoRepo, &pinned_configs(&global_opts))
                {
                    Err(errors::triage_error(&config, err, None))
                } else {
                    Err(err)
                }
            }
        }
    }

    pub fn orig_args(&self) -> &[String] {
        &self.orig_args
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Get a reference to the parsed config.
    pub fn config(&self) -> &Arc<dyn Config> {
        self.optional_repo.config()
    }

    /// Get a reference to the global options.
    pub fn global_opts(&self) -> &HgGlobalOpts {
        &self.early_global_opts
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
            self.optional_repo = OptionalRepo::None(Arc::new(self.load_repoless_config()?));
        }

        Ok(())
    }

    fn load_repoless_config(&self) -> Result<ConfigSet> {
        configloader::hg::load(RepoInfo::NoRepo, &pinned_configs(&self.early_global_opts))
    }

    fn default_command(&self) -> Result<String> {
        // Passing in --version also disables this behavior,
        // but that option is handled somewhere else
        if self.early_global_opts.help || hgplain::is_plain(None) {
            return Err(UnknownCommand(String::new()).into());
        }

        let config = self.optional_repo.config();
        let no_repo_command = config.get_nonempty("commands", "naked-default.no-repo");
        let in_repo_command = config.get_nonempty("commands", "naked-default.in-repo");

        match (
            self.optional_repo.has_repo(),
            no_repo_command,
            in_repo_command,
        ) {
            (false, Some(command), _) => Ok(command.to_string()),
            (true, _, None) => Err(errors::CommandRequired.into()),
            (false, None, None) => Err(UnknownCommand(String::new()).into()),
            (true, _, Some(command)) | (false, None, Some(command)) => Ok(command.to_string()),
        }
    }

    fn prepare_command<'a>(
        &mut self,
        command_table: &'a CommandTable,
        io: &IO,
    ) -> Result<(&'a CommandDefinition, ParseOutput)> {
        let config = self.optional_repo.config();

        if !self.early_global_opts.cwd.is_empty() {
            env::set_current_dir(&self.early_global_opts.cwd)?;
        }

        initialize_libraries(config)?;
        initialize_hgtime(config)?;

        // Prepare alias handling.
        let alias_lookup = |name: &str| {
            // [alias] can have "<name>:doc" entries that are not commands. Skip them.
            if name.contains(':') {
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

        let first_arg = if let Some(first_arg) = self.early_result.args.first() {
            first_arg.clone()
        } else {
            let default_command = self.default_command()?;
            self.args.insert(1, default_command.clone());
            self.early_result.args = vec![default_command.clone()];
            self.early_result.first_arg_index = 0;
            default_command
        };

        let args = self.args[1..].to_vec();
        let early_result = &self.early_result;
        let first_arg_index = early_result.first_arg_index();

        // This should hold true since `first_arg` is not empty (tested above).
        // Therefore positional args is non-empty and first_arg_index should be
        // an index in args.
        debug_assert!(first_arg_index < args.len());
        debug_assert_eq!(args[first_arg_index], first_arg);

        // The difference between args, expanded and new_args is:
        // - args are unchanged arguments provided by the user, unless only global options are provided.
        //   args can have global flags before command name.
        //   for example, ["--traceback", "log", "-Gvr", "master"]
        //                                ^^^^^ first_arg_index, "log" is "command_name"
        // - expanded: includes alias expansion result
        //   no global flags before command name.
        //   for example, with alias "log = log -f", ["log", "-Gvr", "master"]
        //   will be expanded to ["log", "-f", "-Gvr", "master"].
        // - new_args: final args to parse, like expanded with global flags.
        //   ["--traceback", "log", "-f", "-Gvr", "master"].

        // If only global options are provided and some conditions are met (see `self.default_command` for details),
        // then the command/alias provided in commands.naked-default.{in|no}-repo is inserted at the beginning
        // and the difference mentioned above is kept. For instance, if :
        // - The `commands.naked-default.in-repo` config is set to "stj"
        // - The `alias.stj` config is set to "stj = status -Tjson --verbose"
        // - The command is run in a repo,
        // - The original args are ["--verbose", "--traceback"]
        // Then the final values of the variables mentioned above are:
        // - args     => ["stj", "--verbose", "--traceback"]
        // - expanded => ["status", "-Tjson", "--verbose", "--verbose", "--traceback"]
        // - new_args => ["status", "-Tjson", "--verbose", "--verbose", "--traceback"]

        let command_name = first_arg;
        let (expanded, _first_arg_index) = expand_aliases(alias_lookup, &args[first_arg_index..])?;
        let (command_name, command_arg_len) =
            find_command_name(|name| command_table.get(name).is_some(), &expanded)
                .ok_or(errors::UnknownCommand(command_name))?;
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
        last_chance_to_abort(&self.early_global_opts, &global_opts)?;

        // Update pinned config values using the "true" global opts. The early global are
        // parsed conservatively (i.e. incompletely) because they can't read aliases from
        // the config yet, and in general don't know which command is being run yet.
        let pinned_configs = pinned_configs(&global_opts);
        if !pinned_configs.is_empty() {
            let mut config = ConfigSet::wrap(self.config().clone()).named("root:pin");
            set_pinned(&mut config, &pinned_configs)?;
            self.set_config(Arc::new(config));
        }

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

        // Logged directly to sampling since `tracing` doesn't support structured data.
        {
            let opt_names = parsed.specified_opts();
            let _ = sampling::log!(
                target: "command_info",
                positional_args=parsed.args(),
                option_names=opt_names,
                option_values={
                    opt_names
                        .iter()
                        .map(|n| opt_value_to_str(parsed.opts().get(n)))
                        .collect::<Vec<_>>()
                }
            );
        }

        tracing::debug!(target: "command_info", command=handler.legacy_alias().unwrap_or_else(|| handler.main_alias()));

        let res = || -> Result<u8> {
            // This may trigger Python fallback if there are Python hooks.
            let hooks = crate::hooks::Hooks::new(self.config(), io, handler)?;

            tracing::debug!("command handled by a Rust function");

            // Convert to repoless before running the "pre" hook. For repoless commands,
            // we don't want to run hooks from root of incidentally containing repo.
            if matches!(handler.func(), CommandFunc::NoRepo(_)) {
                self.convert_to_repoless_config()?;
            }

            #[cfg(feature = "cas")]
            if handler.enable_cas() {
                cas_client::init();
            }

            hooks.run_pre(self.repo(), &self.args[1..])?;

            let res = match handler.func() {
                CommandFunc::Repo(f) => f(parsed, io, self.repo_mut()?),
                CommandFunc::OptionalRepo(f) => f(parsed, io, &mut self.optional_repo),
                CommandFunc::NoRepo(f) => f(parsed, io, self.optional_repo.config()),
                CommandFunc::WorkingCopy(f) => {
                    let repo = self.repo_mut()?;
                    let wc = repo.working_copy()?;
                    let mut wc = wc.write();
                    f(parsed, io, repo, &mut wc)
                }
            };

            match &res {
                Ok(result_code) => hooks.run_post(self.repo(), &self.args[1..], *result_code)?,
                Err(_) => hooks.run_fail(self.repo(), &self.args[1..])?,
            }

            res
        }();

        (Some(handler), res)
    }

    fn repo_mut(&mut self) -> Result<&mut Repo> {
        match self.optional_repo {
            OptionalRepo::Some(ref mut repo) => Ok(repo),
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

    fn set_config(&mut self, new: Arc<dyn Config>) {
        match &mut self.optional_repo {
            OptionalRepo::Some(repo) => repo.set_config(new),
            OptionalRepo::None(old) => *old = new,
        }
    }
}

fn opt_value_to_str(value: Option<&Value>) -> Cow<'_, str> {
    let opt_str: Option<Cow<str>> = value.and_then(|v| match v {
        Value::Bool(b) => b.map(|b| Cow::Borrowed(if b { "true" } else { "false" })),
        Value::Str(s) => s.as_ref().map(|s| Cow::Borrowed(s.as_ref())),
        Value::Int(i) => i.map(|i| Cow::Owned(i.to_string())),
        Value::List(l) => match l.len() {
            0 => None,
            1 => Some(Cow::Borrowed(&l[0])),
            _ => Some(Cow::Owned(l.join(","))),
        },
    });

    opt_str.unwrap_or(Cow::Borrowed("<unset>"))
}
