// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::command::{CommandDefinition, CommandType};
use crate::errors::{DispatchError, HighLevelError};
use crate::global_flags::HG_GLOBAL_FLAGS;
use crate::io::IO;
use crate::repo::Repo;
use bytes::Bytes;
use cliparser::alias::{expand_aliases, expand_prefix};
use cliparser::parser::*;
use configparser::config::ConfigSet;
use configparser::hg::{parse_list, ConfigSetHgExt};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryInto;
use std::env;
use std::path::{Path, PathBuf};

fn load_config() -> Result<ConfigSet, DispatchError> {
    // priority is ->
    //     - system
    //     - user
    //     - repo
    //     - configfile
    //     - config ( bottom overrides above )
    let mut errors = Vec::new();
    let mut config = ConfigSet::new();
    errors.extend(config.load_system());
    errors.extend(config.load_user());
    if errors.len() > 0 {
        Err(DispatchError::ConfigIssue)
    } else {
        Ok(config)
    }
}

fn load_repo_config(mut config: ConfigSet, current_path: Option<&Path>) -> ConfigSet {
    let mut errors = Vec::new();

    if current_path.is_none() {
        return config;
    }

    let path = current_path.unwrap();

    if let Some(repo_path) = find_hg_repo_root(path) {
        errors.extend(config.load_hgrc(repo_path.join(".hg/hgrc"), "repository"));
    }

    config
}

fn override_config<P>(
    mut config: ConfigSet,
    config_paths: &[P],
    config_overrides: &[String],
) -> Result<ConfigSet, DispatchError>
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
            .ok_or_else(|| DispatchError::ConfigIssue)?;
        let section_name_pair = &config_override[..equals_pos];
        let value = &config_override[equals_pos + 1..];

        let dot_pos = section_name_pair
            .find(".")
            .ok_or_else(|| DispatchError::ConfigIssue)?;
        let section = &section_name_pair[..dot_pos];
        let name = &section_name_pair[dot_pos + 1..];

        config.set(section, name, Some(&Bytes::from(value)), &"--config".into());
    }

    Ok(config)
}

pub fn find_hg_repo_root(current_path: &Path) -> Option<&Path> {
    if current_path.join(".hg").is_dir() {
        Some(current_path)
    } else if let Some(parent) = current_path.parent() {
        find_hg_repo_root(parent)
    } else {
        None
    }
}

pub fn args() -> Result<Vec<String>, DispatchError> {
    let os_args = env::args_os();

    let resultant: Result<Vec<String>, _> = os_args.skip(1).map(|os| os.into_string()).collect();

    resultant.map_err(|_| DispatchError::InvalidCommandLineArguments)
}

pub struct Dispatcher {
    command_table: BTreeMap<String, CommandType>,
    commands: BTreeMap<String, CommandDefinition>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Dispatcher {
            command_table: BTreeMap::new(),
            commands: BTreeMap::new(),
        }
    }

    pub fn add_command(&mut self, command: CommandDefinition) {
        let name = command.name();
        if !self.commands.contains_key(name) {
            self.commands.insert(name.to_string(), command);
        }
    }

    pub fn get_command_table(&self) -> Vec<&CommandDefinition> {
        self.commands.values().collect()
    }

    fn find_command_name(&self, args: Vec<String>) -> Option<String> {
        let mut command_name = None;
        for arg in args {
            if command_name.is_none() {
                if self.command_table.contains_key(&arg) {
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
                if self.command_table.contains_key(&curr) {
                    command_name = Some(curr.to_string())
                } else {
                    return Some(orig);
                }
            }
        }
        command_name
    }

    fn create_repo(repository_path: String) -> Result<Option<Repo>, DispatchError> {
        if repository_path == "" {
            let cwd = env::current_dir().unwrap();
            let root = match find_hg_repo_root(&cwd) {
                Some(r) => r,
                None => return Ok(None),
            };
            return Ok(Some(Repo::new(root)));
        } else if let Ok(repo_path) = Path::new(&repository_path).canonicalize() {
            if repo_path.join(".hg").is_dir() {
                return Ok(Some(Repo::new(repo_path)));
            }
        }
        Err(DispatchError::RepoNotFound {
            path: repository_path,
        })
    }

    fn last_chance_to_abort(opts: &ParseOutput) -> Result<(), DispatchError> {
        if opts.get_or_default("profile", false) {
            return Err(DispatchError::ProfileFlagNotSupported);
        }

        if opts.get_or_default("help", false) {
            return Err(DispatchError::HelpFlagNotSupported);
        }

        Ok(())
    }

    fn early_parse(&self, args: &Vec<String>) -> Result<ParseOutput, DispatchError> {
        let parser = Parser::new(HG_GLOBAL_FLAGS.clone()).with_parsing_options(
            ParseOptions::new()
                .ignore_prefix(true)
                .early_parse(true)
                .flag_alias("repo", "repository"),
        );

        parser
            .parse_args(args)
            .map_err(|_| DispatchError::EarlyParseFailed)
    }

    fn change_workdir(opts: &HashMap<String, Value>) -> Result<(), DispatchError> {
        if let Some(cwd_val) = opts.get("cwd") {
            let cwd: String = cwd_val.clone().into();
            if cwd != "" {
                env::set_current_dir(cwd).map_err(|_| DispatchError::EarlyParseFailed)?;
            }
        }
        Ok(())
    }

    fn repo_from(opts: &HashMap<String, Value>) -> Result<Option<Repo>, DispatchError> {
        match opts.get("repository") {
            Some(repo_val) => {
                let repo_path: String = repo_val.clone().try_into().unwrap();

                Dispatcher::create_repo(repo_path)
            }
            _ => Err(DispatchError::ProgrammingError {
                root_cause: "global flag repository should always be present in options"
                    .to_string(),
            }),
        }
    }

    fn configs(opts: &HashMap<String, Value>) -> Vec<String> {
        opts.get("config")
            .map(|c| c.clone().try_into().unwrap_or(Vec::new()))
            .unwrap_or(Vec::new())
    }

    fn configfiles(opts: &HashMap<String, Value>) -> Vec<PathBuf> {
        opts.get("configfile")
            .map(|c| c.clone().try_into().unwrap_or(Vec::new()))
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|s| PathBuf::from(s))
            .collect()
    }

    fn load_python_commands(&mut self, cfg: &ConfigSet) -> Result<(), DispatchError> {
        let config_commands = parse_list(cfg.get("commands", "names").unwrap_or_default());

        for b_name in config_commands {
            if let Ok(name) = String::from_utf8(b_name.to_vec()) {
                name.trim_start_matches("^")
                    .split("|")
                    .map(|n| CommandDefinition::new(n))
                    .for_each(|c| {
                        self.add_command(c.mark_as_python());
                    });
            }
        }

        Ok(())
    }

    fn command_map(&self, cfg: &ConfigSet) -> BTreeMap<String, isize> {
        let mut command_map = BTreeMap::new();
        let mut i = 1;

        for command in (&self.commands).values() {
            let name = command.name();
            let is_debug = name.starts_with("debug");
            command_map.insert(name.to_string(), if is_debug { -i } else { i });
            i = i + 1;
        }
        // adding aliases into the command map is what Python does, so copying this behavior
        // allows alias expansion to not behave differently for Rust or Python.
        for name in cfg.keys("alias") {
            if let Ok(name) = String::from_utf8(name.to_vec()) {
                let is_debug = name.starts_with("debug");
                command_map.insert(name, if is_debug { -i } else { i });
                i = i + 1;
            }
        }

        command_map
    }

    fn parse(
        &self,
        args: &Vec<String>,
        command_name: &String,
    ) -> Result<ParseOutput, DispatchError> {
        let mut command_flags = self
            .commands
            .get(command_name)
            .map(|command| command.flags().clone())
            .unwrap_or_default();
        command_flags.extend(HG_GLOBAL_FLAGS.clone());
        let parser = Parser::new(command_flags).with_parsing_options(
            ParseOptions::new()
                .error_on_unknown_opts(true)
                .flag_alias("repo", "repository"),
        );

        parser
            .parse_args(args)
            .map_err(|_| DispatchError::ParseFailed)
    }

    pub fn dispatch(&mut self, args: Vec<String>) -> Result<u8, DispatchError> {
        let mut io = IO::default();

        match self._dispatch(args, &mut io) {
            Ok(ret) => {
                return Ok(ret);
            }
            Err(err) => {
                let high_level: HighLevelError = err.into();
                match high_level {
                    HighLevelError::UnsupportedError { cause } => {
                        return Err(cause);
                    }
                    HighLevelError::SupportedError { cause } => {
                        let msg = format!("{}\n", cause);
                        io.write_str(msg)?;
                        return Ok(255);
                    }
                }
            }
        }
    }

    pub fn _dispatch(&mut self, mut args: Vec<String>, io: &mut IO) -> Result<u8, DispatchError> {
        let early_result = self.early_parse(&args)?;

        let early_opts = early_result.opts();

        Dispatcher::change_workdir(&early_opts)?;

        let repo_res = Dispatcher::repo_from(&early_opts);

        let (repo, repo_err) = match repo_res {
            Ok(opt) => (opt, Ok(())),
            Err(err) => (None, Err(err)),
        };

        let config_set = load_config()?;

        let opt_path = repo.as_ref().map(|r| r.path());

        let config_set = load_repo_config(config_set, opt_path);

        let configs = Dispatcher::configs(&early_opts);

        let configfiles = Dispatcher::configfiles(&early_opts);

        let config_set = override_config(config_set, &configfiles[..], &configs)?;

        self.load_python_commands(&config_set)?;

        let alias_lookup = |name: &str| match (
            config_set.get("alias", name),
            config_set.get("defaults", name),
        ) {
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
        };

        let command_map = self.command_map(&config_set);

        let early_args = early_result.args();

        let first_arg = early_args
            .get(0)
            .ok_or_else(|| DispatchError::NoCommandFound)?;

        let replace = early_result.first_arg_index();

        // This should hold true since `first_arg` is not empty (tested above).
        // Therefore positional args is non-empty and first_arg_index should be
        // an index in args.
        debug_assert!(replace < args.len());
        debug_assert_eq!(&args[replace], first_arg);
        // FIXME: DispatchError::AliasExpansionFailed should contain information about
        // ambiguous commands.
        let command_name = expand_prefix(&command_map, first_arg)
            .map_err(|_| DispatchError::AliasExpansionFailed)?;
        args[replace] = command_name;

        let (expanded, _replaced) = expand_aliases(alias_lookup, &args[replace..])
            .map_err(|_| DispatchError::AliasExpansionFailed)?;

        let mut new_args = Vec::new();

        new_args.extend_from_slice(&args[..replace]);
        new_args.extend_from_slice(&expanded[..]);

        let command_name = self
            .find_command_name(expanded)
            .ok_or_else(|| DispatchError::NoCommandFound)?;

        repo_err?;

        let full_args = new_args;

        let result = self.parse(&full_args, &command_name)?;

        Dispatcher::last_chance_to_abort(&result)?;

        let command_length = command_name.split(" ").count();

        let handler = self.command_table.get(&command_name).unwrap();

        let res = match handler {
            CommandType::Repo(f) => {
                let mut r = repo.ok_or_else(|| DispatchError::RepoRequired {
                    cwd: env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or("".to_string()),
                })?;

                r.set_config(config_set);
                let args = result.args().iter().skip(command_length).cloned().collect();
                f(result, args, io, r)
            }
            CommandType::InferRepo(f) => {
                let r = match repo {
                    Some(mut re) => {
                        re.set_config(config_set);
                        Some(re)
                    }
                    None => None,
                };
                let args = args.iter().skip(command_length).cloned().collect();
                f(result, args, io, r)
            }
            CommandType::NoRepo(f) => {
                let args = result.args().clone();
                f(result, args, io)
            }
        };

        res
    }
}

pub trait Register<FN, T> {
    fn register(&mut self, command: CommandDefinition, f: FN);
}

// No Repo
impl<S, FN> Register<FN, (S,)> for Dispatcher
where
    S: From<ParseOutput>,
    FN: Fn(S, Vec<String>, &mut IO) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, command: CommandDefinition, inner_func: FN) {
        let wrapped = move |opts: ParseOutput, args: Vec<String>, io: &mut IO| {
            let translated_opts = opts.into();
            inner_func(translated_opts, args, io)
        };
        self.command_table.insert(
            command.name().to_owned(),
            CommandType::NoRepo(Box::new(wrapped)),
        );
        self.add_command(command);
    }
}

// Infer Repo
impl<S, FN> Register<FN, ((), (((S,),),))> for Dispatcher
where
    S: From<ParseOutput>,
    FN: Fn(S, Vec<String>, &mut IO, Option<Repo>) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, command: CommandDefinition, inner_func: FN) {
        let wrapped =
            move |opts: ParseOutput, args: Vec<String>, io: &mut IO, repo: Option<Repo>| {
                let translated_opts = opts.into();
                inner_func(translated_opts, args, io, repo)
            };
        self.command_table.insert(
            command.name().to_owned(),
            CommandType::InferRepo(Box::new(wrapped)),
        );
        self.add_command(command);
    }
}

// Repo
impl<S, FN> Register<FN, ((), (), ((S,),))> for Dispatcher
where
    S: From<ParseOutput>,
    FN: Fn(S, Vec<String>, &mut IO, Repo) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, command: CommandDefinition, inner_func: FN) {
        let wrapped = move |opts: ParseOutput, args: Vec<String>, io: &mut IO, repo: Repo| {
            let translated_opts = opts.into();
            inner_func(translated_opts, args, io, repo)
        };
        self.command_table.insert(
            command.name().to_owned(),
            CommandType::Repo(Box::new(wrapped)),
        );
        self.add_command(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_config_with_config_overrides_present() {
        let config = ConfigSet::new();
        let config_pairs = vec!["foo.bar=1".to_string(), "bar.foo=2".to_string()];
        let empty: [PathBuf; 0] = [];
        let config = override_config(config, &empty, &config_pairs).unwrap();

        assert_eq!(
            config.sections(),
            vec![Bytes::from("foo"), Bytes::from("bar")]
        );
        assert_eq!(config.keys("foo"), vec![Bytes::from("bar")]);
        assert_eq!(config.keys("bar"), vec![Bytes::from("foo")]);

        assert_eq!(config.get("foo", "bar"), Some(Bytes::from("1")));
        assert_eq!(config.get("bar", "foo"), Some(Bytes::from("2")));

        let sources = config.get_sources("foo", "bar");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--config"));
    }

    #[test]
    fn test_config_with_complex_value() {
        let config = ConfigSet::new();
        let config_pairs = vec!["pager.pager=LESS=FRKX less".to_string()];
        let empty: [PathBuf; 0] = [];
        let config = override_config(config, &empty, &config_pairs).unwrap();

        assert_eq!(config.sections(), vec![Bytes::from("pager")]);

        assert_eq!(config.keys("pager"), vec![Bytes::from("pager")]);
        assert_eq!(
            config.get("pager", "pager"),
            Some(Bytes::from("LESS=FRKX less"))
        );

        let sources = config.get_sources("pager", "pager");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--config"));
    }

    pub(crate) fn write_file(path: PathBuf, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_config_with_configfile_overrides_present() {
        let dir = tempdir().unwrap();

        let config = ConfigSet::new();
        write_file(dir.path().join("foo.rc"), "[foo]\nbar=1\n[bar]\nfoo=2");
        let path = dir.path().join("foo.rc");
        let configfiles = vec![path.as_path()];
        let config = override_config(config, &configfiles, &[]).unwrap();

        assert_eq!(
            config.sections(),
            vec![Bytes::from("foo"), Bytes::from("bar")]
        );
        assert_eq!(config.keys("foo"), vec![Bytes::from("bar")]);
        assert_eq!(config.keys("bar"), vec![Bytes::from("foo")]);

        assert_eq!(config.get("foo", "bar"), Some(Bytes::from("1")));
        assert_eq!(config.get("bar", "foo"), Some(Bytes::from("2")));

        let sources = config.get_sources("foo", "bar");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--configfile"));
    }

    #[test]
    fn test_find_repo_root_found() {
        let dir = tempdir().unwrap();
        let _ = fs::create_dir(dir.path().join(".hg"));
        let path = dir.path().join("a/b/c");
        let hg_root = find_hg_repo_root(&path);
        assert_eq!(dir.path(), hg_root.unwrap());
    }

    #[test]
    fn test_load_repo_config() {
        let dir = tempdir().unwrap();
        let _ = fs::create_dir(dir.path().join(".hg"));
        write_file(dir.path().join(".hg/hgrc"), "[foo]\nbar=1\n[bar]\nfoo=2");
        let config = ConfigSet::new();
        let config = load_repo_config(config, Some(dir.path()));

        assert_eq!(
            config.sections(),
            vec![Bytes::from("foo"), Bytes::from("bar")]
        );

        assert_eq!(config.keys("foo"), vec![Bytes::from("bar")]);
        assert_eq!(config.keys("bar"), vec![Bytes::from("foo")]);

        assert_eq!(config.get("foo", "bar"), Some(Bytes::from("1")));
        assert_eq!(config.get("bar", "foo"), Some(Bytes::from("2")));

        let sources = config.get_sources("foo", "bar");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("repository"));
    }

    #[test]
    fn test_load_repo_config_file_not_found() {
        let dir = tempdir().unwrap();
        let _ = fs::create_dir(dir.path().join(".hg"));
        let config = ConfigSet::new();
        let config = load_repo_config(config, Some(dir.path()));

        assert!(config.sections().len() == 0);
    }

    #[test]
    fn test_find_repo_root_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/c");
        let hg_root = find_hg_repo_root(&path);
        assert!(hg_root.is_none());
    }

}
