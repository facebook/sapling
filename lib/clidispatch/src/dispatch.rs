// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::command::{CommandDefinition, CommandFunc, CommandTable};
use crate::errors::{DispatchError, HighLevelError};
use crate::global_flags::HgGlobalOpts;
use crate::io::IO;
use crate::repo::Repo;
use bytes::Bytes;
use cliparser::alias::{expand_aliases, expand_prefix};
use cliparser::parser::{ParseOptions, ParseOutput, StructFlags, Value};
use configparser::config::ConfigSet;
use configparser::hg::{parse_list, ConfigSetHgExt};
use std::collections::{BTreeMap, HashMap};
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
    if opts.pick::<bool>("profile") {
        return Err(DispatchError::ProfileFlagNotSupported);
    }

    if opts.pick::<bool>("help") {
        return Err(DispatchError::HelpFlagNotSupported);
    }

    Ok(())
}

fn early_parse(args: &Vec<String>) -> Result<ParseOutput, DispatchError> {
    ParseOptions::new()
        .ignore_prefix(true)
        .early_parse(true)
        .flags(HgGlobalOpts::flags())
        .flag_alias("repo", "repository")
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

            create_repo(repo_path)
        }
        _ => Err(DispatchError::ProgrammingError {
            root_cause: "global flag repository should always be present in options".to_string(),
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

fn parse(definition: &CommandDefinition, args: &Vec<String>) -> Result<ParseOutput, DispatchError> {
    let flags = definition
        .flags()
        .iter()
        .chain(HgGlobalOpts::flags().iter())
        .cloned()
        .collect();
    ParseOptions::new()
        .error_on_unknown_opts(true)
        .flags(flags)
        .flag_alias("repo", "repository")
        .parse_args(args)
        .map_err(|_| DispatchError::ParseFailed)
}

pub fn dispatch(command_table: &CommandTable, args: Vec<String>) -> Result<u8, DispatchError> {
    let mut io = IO::stdio();

    match _dispatch(command_table, args, &mut io) {
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
                    io.write(msg)?;
                    return Ok(255);
                }
            }
        }
    }
}

pub fn _dispatch(
    command_table: &CommandTable,
    mut args: Vec<String>,
    io: &mut IO,
) -> Result<u8, DispatchError> {
    let early_result = early_parse(&args)?;

    let early_opts = early_result.opts();

    change_workdir(&early_opts)?;

    let repo_res = repo_from(&early_opts);

    let (repo, repo_err) = match repo_res {
        Ok(opt) => (opt, Ok(())),
        Err(err) => (None, Err(err)),
    };

    let config_set = load_config()?;

    let opt_path = repo.as_ref().map(|r| r.path());

    let config_set = load_repo_config(config_set, opt_path);

    let configs = configs(&early_opts);

    let configfiles = configfiles(&early_opts);

    let config_set = override_config(config_set, &configfiles[..], &configs)?;

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
            if let (Ok(a), Ok(d)) = (String::from_utf8(a.to_vec()), String::from_utf8(d.to_vec())) {
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

    let command_map = command_map(command_table.values(), &config_set);

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
    let command_name =
        expand_prefix(&command_map, first_arg).map_err(|_| DispatchError::AliasExpansionFailed)?;
    args[replace] = command_name;

    let (expanded, _replaced) = expand_aliases(alias_lookup, &args[replace..])
        .map_err(|_| DispatchError::AliasExpansionFailed)?;

    let mut new_args = Vec::new();

    new_args.extend_from_slice(&args[..replace]);
    new_args.extend_from_slice(&expanded[..]);

    let command_name = find_command_name(|name| command_table.contains_key(name), expanded)
        .ok_or_else(|| DispatchError::NoCommandFound)?;

    repo_err?;

    let full_args = new_args;

    let def = &command_table[&command_name];
    let result = parse(&def, &full_args)?;

    last_chance_to_abort(&result)?;

    let handler = def.func();

    match handler {
        CommandFunc::Repo(f) => {
            let mut r = repo.ok_or_else(|| DispatchError::RepoRequired {
                cwd: env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or("".to_string()),
            })?;

            r.set_config(config_set);
            f(result, io, r)
        }
        CommandFunc::InferRepo(f) => {
            let r = match repo {
                Some(mut re) => {
                    re.set_config(config_set);
                    Some(re)
                }
                None => None,
            };
            f(result, io, r)
        }
        CommandFunc::NoRepo(f) => f(result, io),
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
