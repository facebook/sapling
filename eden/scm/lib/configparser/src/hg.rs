/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mercurial-specific config postprocessing

use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs;
#[cfg(feature = "fb")]
use std::fs::read_to_string;
use std::hash::Hash;
use std::io;
use std::io::Error as IOError;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use minibytes::Text;
use url::Url;
use util::path::expand_path;

use crate::config::ConfigSet;
use crate::config::Options;
use crate::config::SupersetVerification;
use crate::error::Error;
use crate::error::Errors;

pub const HGPLAIN: &str = "HGPLAIN";
pub const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";
pub const HGRCPATH: &str = "HGRCPATH";

pub trait OptionsHgExt {
    /// Drop configs according to `$HGPLAIN` and `$HGPLAINEXCEPT`.
    fn process_hgplain(self) -> Self;

    /// Set read-only config items. `items` contains a list of tuple `(section, name)`.
    /// Setting those items to new value will be ignored.
    fn readonly_items<S: Into<Text>, N: Into<Text>>(self, items: Vec<(S, N)>) -> Self;

    /// Set section remap. If a section name matches an entry key, it will be treated as if the
    /// name is the entry value. The remap wouldn't happen recursively. For example, with a
    /// `{"A": "B", "B": "C"}` map, section name "A" will be treated as "B", not "C".
    /// This is implemented via `append_filter`.
    fn remap_sections<K: Eq + Hash + Into<Text>, V: Into<Text>>(self, remap: HashMap<K, V>)
    -> Self;

    /// Filter sections. Sections outside include_sections won't be loaded.
    /// This is implemented via `append_filter`.
    fn filter_sections<B: Clone + Into<Text>>(self, include_sections: Vec<B>) -> Self;
}

pub trait ConfigSetHgExt {
    fn load<S: Into<Text>, N: Into<Text>>(
        &mut self,
        repo_path: Option<&Path>,
        readonly_items: Option<Vec<(S, N)>>,
    ) -> Result<SupersetVerification, Errors>;

    /// Load system config files if `$HGRCPATH` is not set.
    /// Return errors parsing files.
    fn load_system(&mut self, opts: Options) -> Vec<Error>;

    #[cfg(feature = "fb")]
    /// Load the dynamic config files for the given repo path.
    /// Returns errors parsing, generating, or fetching the configs.
    fn load_dynamic(&mut self, repo_path: Option<&Path>, opts: Options) -> Result<Vec<Error>>;

    /// Load user config files (and environment variables).  If `$HGRCPATH` is
    /// set, load files listed in that environment variable instead.
    /// Return errors parsing files.
    fn load_user(&mut self, opts: Options) -> Vec<Error>;

    /// Load repo config files.
    fn load_repo(&mut self, repo_path: &Path, opts: Options) -> Vec<Error>;

    /// Load a specified config file. Respect HGPLAIN environment variables.
    /// Return errors parsing files.
    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error>;

    fn validate_dynamic(&mut self) -> Result<SupersetVerification, Error>;
}

/// Load config from specified repo .hg path, or global config if no path specified.
/// `extra_values` contains config overrides (i.e. "--config" CLI values).
/// `extra_files` contains additional config files (i.e. "--configfile" CLI values).
pub fn load(
    repo_path: Option<&Path>,
    extra_values: &[String],
    extra_files: &[String],
) -> Result<ConfigSet> {
    let mut cfg = ConfigSet::new();

    let mut errors = Vec::new();
    for path in extra_files {
        errors.extend(cfg.load_path(&path, &"--configfile".into()));
    }

    if let Err(err) = cfg.set_overrides(extra_values) {
        errors.push(err);
    }

    match cfg.load::<Text, Text>(repo_path, None) {
        Ok(_) => {
            if !errors.is_empty() {
                return Err(Errors(errors).into());
            }
        }
        Err(mut err) => {
            err.0.extend(errors);
            return Err(err.into());
        }
    }

    // Load the CLI configs again to make sure they take precedence.
    // The "readonly" facility can't be used to pin the configs
    // because it doesn't interact with the config verification properly.
    for path in extra_files {
        cfg.load_path(&path, &"--configfile".into());
    }

    let _ = cfg.set_overrides(extra_values);

    Ok(cfg)
}

impl OptionsHgExt for Options {
    fn process_hgplain(self) -> Self {
        let plain_set = env::var(HGPLAIN).is_ok();
        let plain_except = env::var(HGPLAINEXCEPT);
        if plain_set || plain_except.is_ok() {
            let (section_exclude_list, ui_exclude_list) = {
                let plain_exceptions: HashSet<String> = plain_except
                    .unwrap_or_else(|_| "".to_string())
                    .split(',')
                    .map(|s| s.to_string())
                    .collect();

                // [defaults] and [commands] are always excluded.
                let mut section_exclude_list: HashSet<Text> =
                    ["defaults", "commands"].iter().map(|&s| s.into()).collect();

                // [alias], [revsetalias], [templatealias] are excluded if they are outside
                // HGPLAINEXCEPT.
                for name in ["alias", "revsetalias", "templatealias"] {
                    if !plain_exceptions.contains(name) {
                        section_exclude_list.insert(Text::from(name));
                    }
                }

                // These configs under [ui] are always excluded.
                let mut ui_exclude_list: HashSet<Text> = [
                    "debug",
                    "fallbackencoding",
                    "quiet",
                    "slash",
                    "logtemplate",
                    "statuscopies",
                    "style",
                    "traceback",
                    "verbose",
                ]
                .iter()
                .map(|&s| s.into())
                .collect();
                // exitcodemask is excluded if exitcode is outside HGPLAINEXCEPT.
                if !plain_exceptions.contains("exitcode") {
                    ui_exclude_list.insert("exitcodemask".into());
                }

                (section_exclude_list, ui_exclude_list)
            };

            let filter = move |section: Text, name: Text, value: Option<Text>| {
                if section_exclude_list.contains(&section)
                    || (section.as_ref() == "ui" && ui_exclude_list.contains(&name))
                {
                    None
                } else {
                    Some((section, name, value))
                }
            };

            self.append_filter(Box::new(filter))
        } else {
            self
        }
    }

    /// Filter sections. Sections outside of include_sections won't be loaded.
    /// This is implemented via `append_filter`.
    fn filter_sections<B: Clone + Into<Text>>(self, include_sections: Vec<B>) -> Self {
        let include_list: HashSet<Text> = include_sections
            .iter()
            .cloned()
            .map(|section| section.into())
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
            if include_list.contains(&section) {
                Some((section, name, value))
            } else {
                None
            }
        };

        self.append_filter(Box::new(filter))
    }

    /// Set section remap. If a section name matches an entry key, it will be treated as if the
    /// name is the entry value. The remap wouldn't happen recursively. For example, with a
    /// `{"A": "B", "B": "C"}` map, section name "A" will be treated as "B", not "C".
    /// This is implemented via `append_filter`.
    fn remap_sections<K, V>(self, remap: HashMap<K, V>) -> Self
    where
        K: Eq + Hash + Into<Text>,
        V: Into<Text>,
    {
        let remap: HashMap<Text, Text> = remap
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
            let section = remap.get(&section).cloned().unwrap_or(section);
            Some((section, name, value))
        };

        self.append_filter(Box::new(filter))
    }

    fn readonly_items<S: Into<Text>, N: Into<Text>>(self, items: Vec<(S, N)>) -> Self {
        let readonly_items: HashSet<(Text, Text)> = items
            .into_iter()
            .map(|(section, name)| (section.into(), name.into()))
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
            if readonly_items.contains(&(section.clone(), name.clone())) {
                None
            } else {
                Some((section, name, value))
            }
        };

        self.append_filter(Box::new(filter))
    }
}

impl ConfigSetHgExt for ConfigSet {
    /// Load system, user config files.
    fn load<S: Into<Text>, N: Into<Text>>(
        &mut self,
        repo_path: Option<&Path>,
        readonly_items: Option<Vec<(S, N)>>,
    ) -> Result<SupersetVerification, Errors> {
        let mut errors = vec![];

        let mut opts = Options::new();
        if let Some(readonly_items) = readonly_items {
            opts = opts.readonly_items(readonly_items);
        }

        errors.append(&mut self.parse(HG_PY_CORE_CONFIG, &opts.clone().source("builtin.rc")));

        // Only load builtin configs if HGRCPATH is not set.
        if std::env::var(HGRCPATH).is_err() {
            errors.append(
                &mut self.parse(MERGE_TOOLS_CONFIG, &opts.clone().source("merge-tools.rc")),
            );
        }
        #[cfg(feature = "fb")]
        errors.append(
            &mut self
                .load_dynamic(repo_path, opts.clone())
                .map_err(|e| Errors(vec![Error::Other(e)]))?,
        );
        errors.append(&mut self.load_system(opts.clone()));
        errors.append(&mut self.load_user(opts.clone()));

        if let Some(repo_path) = repo_path {
            errors.append(&mut self.load_repo(&repo_path, opts.clone()));
            if let Err(e) = read_set_repo_name(self, repo_path) {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(Errors(errors));
        }

        self.validate_dynamic().map_err(|err| Errors(vec![err]))
    }

    fn load_system(&mut self, opts: Options) -> Vec<Error> {
        let opts = opts.source("system").process_hgplain();
        let mut errors = Vec::new();

        // If $HGRCPATH is set, use it instead.
        if let Ok(rcpath) = env::var("HGRCPATH") {
            #[cfg(unix)]
            let paths = rcpath.split(':');
            #[cfg(windows)]
            let paths = rcpath.split(';');
            for path in paths {
                errors.append(&mut self.load_path(expand_path(path), &opts));
            }
        } else {
            #[cfg(unix)]
            {
                errors.append(&mut self.load_path("/etc/mercurial/system.rc", &opts));
                // TODO(T40519286): Remove this after the tupperware overrides move out of hgrc.d
                errors.append(
                    &mut self.load_path("/etc/mercurial/hgrc.d/tupperware_overrides.rc", &opts),
                );
                // TODO(quark): Remove this after packages using system.rc are rolled out
                errors.append(&mut self.load_path("/etc/mercurial/hgrc.d/include.rc", &opts));
            }

            #[cfg(windows)]
            {
                if let Ok(program_data_path) = env::var("PROGRAMDATA") {
                    let hgrc_dir = Path::new(&program_data_path).join("Facebook\\Mercurial");
                    errors.append(&mut self.load_path(hgrc_dir.join("system.rc"), &opts));
                    // TODO(quark): Remove this after packages using system.rc are rolled out
                    errors.append(&mut self.load_path(hgrc_dir.join("hgrc"), &opts));
                }
            }
        }

        errors
    }

    #[cfg(feature = "fb")]
    fn load_dynamic(&mut self, repo_path: Option<&Path>, opts: Options) -> Result<Vec<Error>> {
        use std::process::Command;
        use std::time::Duration;
        use std::time::SystemTime;

        use anyhow::bail;
        use util::run_background;

        let mut errors = Vec::new();

        // Compute path
        let dynamic_path = get_config_dir(repo_path)?.join("hgrc.dynamic");

        // Check version
        let content = read_to_string(&dynamic_path).ok();
        let version = content.as_ref().and_then(|c| {
            let mut lines = c.split("\n");
            match lines.next() {
                Some(line) if line.starts_with("# version=") => Some(&line[10..]),
                Some(_) | None => None,
            }
        });

        let this_version = ::version::VERSION;

        // Synchronously generate the new config if it's out of date with our version
        if version != Some(this_version) {
            tracing::info!("dynamicconfig: regenerate at {}", dynamic_path.display());
            let (repo_name, user_name) = {
                let mut temp_config = ConfigSet::new();
                if !temp_config.load_user(opts.clone()).is_empty() {
                    bail!("unable to read user config to get user name");
                }

                let repo_name = match repo_path {
                    Some(repo_path) => {
                        let opts = opts.clone().source("temp").process_hgplain();
                        // We need to know the repo name, but that's stored in the repository configs at
                        // the moment. In the long term we need to move that, but for now let's load the
                        // repo config ahead of time to read the name.
                        let repo_hgrc_path = repo_path.join("hgrc");
                        if !temp_config.load_path(repo_hgrc_path, &opts).is_empty() {
                            bail!("unable to read repo config to get repo name");
                        }
                        Some(read_set_repo_name(&mut temp_config, repo_path)?)
                    }
                    None => None,
                };

                (repo_name, temp_config.get_or_default("ui", "username")?)
            };

            // Regen inline
            let res = generate_dynamicconfig(repo_path, repo_name, None, user_name);
            if let Err(e) = res {
                let is_perm_error = e
                    .chain()
                    .any(|cause| match cause.downcast_ref::<IOError>() {
                        Some(io_error) if io_error.kind() == ErrorKind::PermissionDenied => true,
                        _ => false,
                    });
                if !is_perm_error {
                    return Err(e);
                }
            }
        }

        if !dynamic_path.exists() {
            return Err(IOError::new(
                ErrorKind::NotFound,
                format!("required config not found at {:?}", dynamic_path),
            )
            .into());
        }

        // Read hgrc.dynamic
        let opts = opts.source("dynamic").process_hgplain();
        errors.append(&mut self.load_path(&dynamic_path, &opts));

        // Log config ages
        // - Done in python for now

        // Regenerate if mtime is old.
        let generation_time: Option<u64> = self.get_opt("configs", "generationtime")?;
        let recursion_marker = env::var("HG_DEBUGDYNAMICCONFIG");
        let mut skip_reason = None;

        if recursion_marker.is_err() {
            if let Some(generation_time) = generation_time {
                let generation_time = Duration::from_secs(generation_time);
                let mtime_age = SystemTime::now()
                    .duration_since(dynamic_path.metadata()?.modified()?)
                    // An error from duration_since means 'now' is older than
                    // 'last_modified'. In that case, let's assume the file
                    // is brand new and has an age of 0.
                    .unwrap_or(Duration::from_secs(0));
                if mtime_age > generation_time {
                    let config_regen_command: Vec<String> =
                        self.get_or("configs", "regen-command", || {
                            vec!["hg".to_string(), "debugdynamicconfig".to_string()]
                        })?;
                    tracing::debug!(
                        "spawn {:?} because mtime {:?} > generation_time {:?}",
                        &config_regen_command,
                        mtime_age,
                        generation_time
                    );
                    if !config_regen_command.is_empty() {
                        let mut command = Command::new(&config_regen_command[0]);
                        command
                            .args(&config_regen_command[1..])
                            .env("HG_DEBUGDYNAMICCONFIG", "1");

                        if let Some(repo_path) = repo_path {
                            command.current_dir(&repo_path);
                        }

                        let _ = run_background(command);
                    }
                } else {
                    skip_reason = Some("mtime <= configs.generationtime");
                }
            } else {
                skip_reason = Some("configs.generationtime is not set");
            }
        } else {
            skip_reason = Some("HG_DEBUGDYNAMICCONFIG is set");
        }
        if let Some(reason) = skip_reason {
            tracing::debug!("skip spawning debugdynamicconfig because {}", reason);
        }

        Ok(errors)
    }

    fn load_user(&mut self, opts: Options) -> Vec<Error> {
        // If HGRCPATH is set, don't load user configs
        let mut paths = Vec::new();
        if env::var("HGRCPATH").is_err() {
            if let Some(home_dir) = dirs::home_dir() {
                paths.push(home_dir.join(".hgrc"));

                #[cfg(windows)]
                {
                    paths.push(home_dir.join("mercurial.ini"));
                }
            }
            if let Some(config_dir) = dirs::config_dir() {
                paths.push(config_dir.join("hg/hgrc"))
            }
        }
        self.load_user_internal(&paths, opts)
    }

    fn load_repo(&mut self, repo_path: &Path, opts: Options) -> Vec<Error> {
        let mut errors = Vec::new();

        let opts = opts.source("repo").process_hgplain();

        let hgrc_path = repo_path.join("hgrc");
        errors.append(&mut self.load_path(hgrc_path, &opts));

        errors
    }

    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error> {
        let opts = Options::new().source(source).process_hgplain();
        self.load_path(path, &opts)
    }

    fn validate_dynamic(&mut self) -> Result<SupersetVerification, Error> {
        let superset_location: String = "hgrc.dynamic".to_string();
        let subset_locations: Vec<String> =
            self.get_or("configs", "validationsubset", || vec![])?;
        let allowed_locations: Option<Vec<String>> =
            self.get_opt::<Vec<String>>("configs", "allowedlocations")?;
        let allowed_configs: Option<Vec<String>> =
            self.get_opt::<Vec<String>>("configs", "allowedconfigs")?;

        Ok(self.ensure_location_supersets(
            superset_location,
            subset_locations,
            allowed_locations
                .as_ref()
                .map(|v| HashSet::from_iter(v.iter().map(|s| s.as_str()))),
            allowed_configs.as_ref().map(|v| {
                HashSet::from_iter(v.iter().map(|s| {
                    let split: Vec<&str> = s.splitn(2, ".").into_iter().collect();
                    (split[0], split[1])
                }))
            }),
        ))
    }
}

/// Read repo name from various places (remotefilelog.reponame, paths.default, .hg/reponame).
///
/// Try to write the reponame back to `.hg/reponame`, and set `remotefilelog.reponame`
/// for code paths using them.
///
/// If `configs.forbid-empty-reponame` is `true`, raise if the repo name is empty
/// and `paths.default` is set.
fn read_set_repo_name(config: &mut ConfigSet, repo_path: &Path) -> crate::Result<String> {
    let (repo_name, source): (String, &str) = {
        let mut name: String = config.get_or_default("remotefilelog", "reponame")?;
        let mut source = "remotefilelog.reponame";
        if name.is_empty() {
            tracing::warn!("repo name: no remotefilelog.reponame");
            let path: String = config.get_or_default("paths", "default")?;
            name = repo_name_from_url(&path).unwrap_or_default();
            if name.is_empty() {
                tracing::warn!("repo name: no path.default reponame: {}", &path);
            }
            source = "paths.default";
        }
        if name.is_empty() {
            match read_repo_name_from_disk(repo_path) {
                Ok(s) => {
                    name = s;
                    source = ".hg/reponame";
                }
                Err(e) => {
                    tracing::warn!("repo name: no .hg/reponame: {:?}", &e);
                }
            };
        }
        (name, source)
    };

    if !repo_name.is_empty() {
        tracing::debug!("repo name: {:?} (from {})", &repo_name, source);
        if source != ".hg/reponame" {
            let need_rewrite = match read_repo_name_from_disk(repo_path) {
                Ok(s) => s != repo_name,
                Err(_) => true,
            };
            if need_rewrite {
                let path = get_repo_name_path(repo_path);
                match fs::write(&path, &repo_name) {
                    Ok(_) => tracing::debug!("repo name: written to .hg/reponame"),
                    Err(e) => tracing::warn!("repo name: cannot write to .hg/reponame: {:?}", e),
                }
            }
        }
        if source != "remotefilelog.reponame" {
            config.set(
                "remotefilelog",
                "reponame",
                Some(&repo_name),
                &source.into(),
            );
        }
    } else {
        let forbid_empty_reponame: bool =
            config.get_or_default("configs", "forbid-empty-reponame")?;
        if forbid_empty_reponame && config.get("paths", "default").is_some() {
            let msg = "reponame is empty".to_string();
            return Err(Error::General(msg));
        }
    }

    Ok(repo_name)
}

impl ConfigSet {
    // For easier testing.
    pub(crate) fn load_user_internal(&mut self, paths: &[PathBuf], opts: Options) -> Vec<Error> {
        let mut errors = Vec::new();

        // Covert "$VISUAL", "$EDITOR" to "ui.editor".
        //
        // Unlike Mercurial, don't convert the "$PAGER" environment variable
        // to "pager.pager" config.
        //
        // The environment variable could be from the system profile (ex.
        // /etc/profile.d/...), or the user shell rc (ex. ~/.bashrc). There is
        // no clean way to tell which one it is from.  The value might be
        // tweaked for sysadmin usecases (ex. -n), which are different from
        // SCM's usecases.
        for name in ["VISUAL", "EDITOR"] {
            if let Ok(editor) = env::var(name) {
                self.set(
                    "ui",
                    "editor",
                    Some(editor),
                    &opts.clone().source(format!("${}", name)),
                );
                break;
            }
        }

        // Convert $HGPROF to profiling.type
        if let Ok(profiling_type) = env::var("HGPROF") {
            self.set("profiling", "type", Some(profiling_type), &"$HGPROF".into());
        }

        let opts = opts.source("user").process_hgplain();

        for path in paths {
            errors.append(&mut self.load_path(path, &opts));
        }

        // Override ui.merge:interactive (source != user) with ui.merge
        // (source == user). This makes ui.merge in user hgrc effective,
        // even if ui.merge:interactive is not set.
        if self
            .get_sources("ui", "merge:interactive")
            .last()
            .map(|s| s.source().as_ref())
            != Some("user")
            && self
                .get_sources("ui", "merge")
                .last()
                .map(|s| s.source().as_ref())
                == Some("user")
        {
            if let Some(merge) = self.get("ui", "merge") {
                self.set("ui", "merge:interactive", Some(merge), &opts);
            }
        }

        errors
    }
}

pub fn repo_name_from_url(s: &str) -> Option<String> {
    // Use a base_url to support non-absolute urls.
    let base_url = Url::parse("file:///.").unwrap();
    let parse_opts = Url::options().base_url(Some(&base_url));
    match parse_opts.parse(s) {
        Ok(url) => {
            tracing::trace!("parsed url {}: {:?}", s, url);
            // Try the last segment in url path.
            if let Some(last_segment) = url
                .path_segments()
                .and_then(|s| s.rev().find(|s| !s.is_empty()))
            {
                return Some(last_segment.to_string());
            }
            // Try path. `path_segment` can be `None` for URL like "test:reponame".
            let path = url.path();
            if !path.contains('/') && !path.is_empty() {
                return Some(path.to_string());
            }
            // Try the hostname. ex. in "fb://fbsource", "fbsource" is a host not a path.
            // Also see https://www.mercurial-scm.org/repo/hg/help/schemes
            if let Some(host_str) = url.host_str() {
                return Some(host_str.to_string());
            }
        }
        Err(e) => {
            tracing::warn!("cannot parse url {}: {:?}", s, e);
        }
    }
    None
}

#[cfg(feature = "fb")]
fn get_config_dir(repo_path: Option<&Path>) -> Result<PathBuf, Error> {
    Ok(match repo_path {
        Some(repo_path) => {
            let shared_path = repo_path.join("sharedpath");
            if shared_path.exists() {
                let raw = read_to_string(&shared_path).map_err(|e| Error::Io(shared_path, e))?;
                let trimmed = raw.trim_end_matches("\n");
                // sharedpath can be relative, so join it with repo_path.
                repo_path.join(trimmed)
            } else {
                repo_path.to_path_buf()
            }
        }
        None => {
            let dirs = vec![
                std::env::var("TESTTMP")
                    .ok()
                    .map(|d| PathBuf::from(d).join(".cache")),
                std::env::var("HG_CONFIG_CACHE_DIR").ok().map(PathBuf::from),
                dirs::cache_dir(),
                Some(std::env::temp_dir()),
            ];

            let mut errs = vec![];
            for mut dir in dirs.into_iter().flatten() {
                dir.push("edenscm");
                match util::path::create_shared_dir_all(&dir) {
                    Err(err) => {
                        tracing::debug!("error setting up config cache dir {:?}: {}", dir, err);
                        errs.push((dir, err));
                        continue;
                    }
                    Ok(()) => return Ok(dir),
                }
            }

            return Err(Error::General(format!(
                "couldn't find config cache dir: {:?}",
                errs
            )));
        }
    })
}

#[cfg(feature = "fb")]
pub fn calculate_dynamicconfig(
    config_dir: PathBuf,
    repo_name: Option<impl AsRef<str>>,
    canary: Option<String>,
    user_name: String,
) -> Result<ConfigSet> {
    use crate::fb::dynamicconfig::Generator;
    Generator::new(repo_name, config_dir, user_name)?.execute(canary)
}

#[cfg(feature = "fb")]
pub fn generate_dynamicconfig(
    repo_path: Option<&Path>,
    repo_name: Option<impl AsRef<str>>,
    canary: Option<String>,
    user_name: String,
) -> Result<()> {
    use std::io::Write;

    use filetime::set_file_mtime;
    use filetime::FileTime;
    use tempfile::tempfile_in;

    tracing::debug!(
        repo_path = ?repo_path,
        canary = ?canary,
        "generate_dynamicconfig",
    );

    // Resolve sharedpath
    let config_dir = get_config_dir(repo_path)?;

    // Verify that the filesystem is writable, otherwise exit early since we won't be able to write
    // the config.
    if tempfile_in(&config_dir).is_err() {
        return Err(IOError::new(
            ErrorKind::PermissionDenied,
            format!("no write access to {:?}", config_dir),
        )
        .into());
    }

    let version = ::version::VERSION;
    let header = format!(
        concat!(
            "# version={}\n",
            "# reponame={}\n",
            "# canary={:?}\n",
            "# username={}\n",
            "# Generated by `hg debugdynamicconfig` - DO NOT MODIFY\n",
        ),
        version,
        repo_name.as_ref().map_or("no_repo", |r| r.as_ref()),
        canary.as_ref(),
        &user_name,
    );

    let hgrc_path = config_dir.join("hgrc.dynamic");
    let global_config_dir = get_config_dir(None)?;

    let config = calculate_dynamicconfig(global_config_dir, repo_name, canary, user_name)?;
    let config_str = format!("{}{}", header, config.to_string());

    // If the file exists and will be unchanged, just update the mtime.
    if hgrc_path.exists() && read_to_string(&hgrc_path).unwrap_or_default() == config_str {
        let time = FileTime::now();
        tracing::debug!("bump {:?} mtime to {:?}", &hgrc_path, &time);
        set_file_mtime(hgrc_path, time)?;
    } else {
        tracing::debug!("rewrite {:?}", &hgrc_path);
        util::file::atomic_write(&hgrc_path, |f| {
            f.write_all(config_str.as_bytes())?;
            Ok(())
        })?;
    }

    Ok(())
}

/// Get the path of the reponame file.
fn get_repo_name_path(shared_dot_hg_path: &Path) -> PathBuf {
    shared_dot_hg_path.join("reponame")
}

/// Read repo name from shared `.hg` path.
pub fn read_repo_name_from_disk(shared_dot_hg_path: &Path) -> io::Result<String> {
    let repo_name_path = get_repo_name_path(shared_dot_hg_path);
    let name = fs::read_to_string(&repo_name_path)?.trim().to_string();
    if name.is_empty() {
        Err(IOError::new(
            ErrorKind::InvalidData,
            format!("reponame could not be empty ({})", repo_name_path.display()),
        ))
    } else {
        Ok(name)
    }
}

/// Return whether plain mode is active, similar to python ui.plain().
pub fn is_plain(feature: Option<&str>) -> bool {
    let plain = env::var(HGPLAIN);
    let plain_except = env::var(HGPLAINEXCEPT);

    if plain.is_err() && plain_except.is_err() {
        return false;
    }

    if let Some(feature) = feature {
        !plain_except
            .unwrap_or_default()
            .split(',')
            .any(|s| s == feature)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use tempdir::TempDir;

    use super::*;
    use crate::config::tests::write_file;
    use crate::lock_env;

    #[test]
    fn test_basic_hgplain() {
        let mut env = lock_env();

        env.set(HGPLAIN, Some("1"));
        env.set(HGPLAINEXCEPT, None);

        let opts = Options::new().process_hgplain();
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[defaults]\n\
             commit = commit -d 0\n\
             [ui]\n\
             verbose = true\n\
             username = test\n\
             [alias]\n\
             l = log\n",
            &opts,
        );

        assert!(cfg.keys("defaults").is_empty());
        assert_eq!(cfg.get("ui", "verbose"), None);
        assert_eq!(cfg.get("ui", "username"), Some("test".into()));
        assert_eq!(cfg.get("alias", "l"), None);
    }

    #[test]
    fn test_hgplainexcept() {
        let mut env = lock_env();

        env.set(HGPLAIN, None);
        env.set(HGPLAINEXCEPT, Some("alias,revsetalias"));

        let opts = Options::new().process_hgplain();
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[defaults]\n\
             commit = commit -d 0\n\
             [alias]\n\
             l = log\n\
             [templatealias]\n\
             u = user\n\
             [revsetalias]\n\
             @ = master\n",
            &opts,
        );

        assert!(cfg.keys("defaults").is_empty());
        assert_eq!(cfg.get("alias", "l"), Some("log".into()));
        assert_eq!(cfg.get("revsetalias", "@"), Some("master".into()));
        assert_eq!(cfg.get("templatealias", "u"), None);
    }

    #[test]
    fn test_is_plain() {
        let mut env = lock_env();

        env.set(HGPLAIN, None);
        env.set(HGPLAINEXCEPT, None);
        assert!(!is_plain(None));

        env.set(HGPLAIN, Some("1"));
        assert!(is_plain(None));
        assert!(is_plain(Some("banana")));

        env.set(HGPLAINEXCEPT, Some("dog,banana,tree"));
        assert!(!is_plain(Some("banana")));

        env.set(HGPLAIN, None);
        assert!(!is_plain(Some("banana")));
    }

    #[test]
    fn test_hgrcpath() {
        let mut env = crate::lock_env();

        let dir = TempDir::new("test_hgrcpath").unwrap();

        write_file(dir.path().join("1.rc"), "[x]\na=1");
        write_file(dir.path().join("2.rc"), "[y]\nb=2");

        #[cfg(unix)]
        let hgrcpath = "$T/1.rc:$T/2.rc";
        #[cfg(windows)]
        let hgrcpath = "$T/1.rc;%T%/2.rc";

        env.set("EDITOR", None);
        env.set("VISUAL", None);
        env.set("HGPROF", None);

        env.set("T", Some(dir.path().to_str().unwrap()));
        env.set(HGRCPATH, Some(hgrcpath));

        let mut cfg = ConfigSet::new();

        cfg.load_user(Options::new());
        assert!(
            cfg.sections().is_empty(),
            "sections {:?} is not empty",
            cfg.sections()
        );

        cfg.load_system(Options::new());
        assert_eq!(cfg.get("x", "a"), Some("1".into()));
        assert_eq!(cfg.get("y", "b"), Some("2".into()));
    }

    #[test]
    fn test_load_user() {
        let _env = lock_env();

        let dir = TempDir::new("test_hgrcpath").unwrap();
        let path = dir.path().join("1.rc");

        write_file(path.clone(), "[ui]\nmerge=x");

        let mut cfg = ConfigSet::new();
        cfg.load_user_internal(&[path.clone()], Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "x");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge", Some("foo"), &"system".into());
        cfg.set("ui", "merge:interactive", Some("foo"), &"system".into());
        cfg.load_user_internal(&[path.clone()], Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "x");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge:interactive", Some("foo"), &"system".into());
        write_file(path.clone(), "[ui]\nmerge=x\nmerge:interactive=y\n");
        cfg.load_user_internal(&[path.clone()], Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "y");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge", Some("a"), &"system".into());
        cfg.set("ui", "merge:interactive", Some("b"), &"system".into());
        write_file(path.clone(), "");
        cfg.load_user_internal(&[path.clone()], Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "a");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "b");
        write_file(path.clone(), "[ui]\nmerge:interactive=y\n");
        cfg.load_user_internal(&[path.clone()], Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "a");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "y");

        drop(path);
    }

    #[test]
    fn test_load_hgrc() {
        let dir = TempDir::new("test_hgrcpath").unwrap();
        let path = dir.path().join("1.rc");

        write_file(path.clone(), "[x]\na=1\n[alias]\nb=c\n");

        let mut env = lock_env();

        env.set(HGPLAIN, Some("1"));
        env.set(HGPLAINEXCEPT, None);

        let mut cfg = ConfigSet::new();
        cfg.load_hgrc(&path, "hgrc");

        assert!(cfg.keys("alias").is_empty());
        assert!(cfg.get("alias", "b").is_none());
        assert_eq!(cfg.get("x", "a").unwrap(), "1");

        env.set(HGPLAIN, None);
        cfg.load_hgrc(&path, "hgrc");

        assert_eq!(cfg.get("alias", "b").unwrap(), "c");
    }

    #[test]
    fn test_section_filter() {
        let opts = Options::new().filter_sections(vec!["x", "y"]);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.sections(), vec![Text::from("x"), Text::from("y")]);
        assert_eq!(cfg.get("z", "c"), None);
    }

    #[test]
    fn test_section_remap() {
        let mut remap = HashMap::new();
        remap.insert("x", "y");
        remap.insert("y", "z");

        let opts = Options::new().remap_sections(remap);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.get("y", "a"), Some("1".into()));
        assert_eq!(cfg.get("z", "b"), Some("2".into()));
        assert_eq!(cfg.get("z", "c"), Some("3".into()));
    }

    #[test]
    fn test_readonly_items() {
        let opts = Options::new().readonly_items(vec![("x", "a"), ("y", "b")]);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.get("x", "a"), None);
        assert_eq!(cfg.get("y", "b"), None);
        assert_eq!(cfg.get("z", "c"), Some("3".into()));
    }

    #[test]
    fn test_py_core_items() {
        let mut cfg = ConfigSet::new();
        cfg.load::<String, String>(None, None).unwrap();
        assert_eq!(cfg.get("treestate", "repackfactor").unwrap(), "3");
    }

    #[test]
    fn test_load_cli_args() {
        let mut env = lock_env();

        // Skip real dynamic config.
        env.set("TESTTMP", Some("1"));

        let dir = TempDir::new("test_load").unwrap();

        let repo_rc = dir.path().join("hgrc");
        write_file(repo_rc, "[s]\na=orig\nb=orig\nc=orig");

        let other_rc = dir.path().join("other.rc");
        write_file(other_rc.clone(), "[s]\na=other\nb=other");

        let cfg = load(
            Some(dir.path()),
            &["s.b=flag".to_string()],
            &[format!("{}", other_rc.display())],
        )
        .unwrap();

        assert_eq!(cfg.get("s", "a"), Some("other".into()));
        assert_eq!(cfg.get("s", "b"), Some("flag".into()));
        assert_eq!(cfg.get("s", "c"), Some("orig".into()));
    }
}

const MERGE_TOOLS_CONFIG: &str = r#"# Some default global settings for common merge tools

[merge-tools]
kdiff3.args=--auto --L1 base --L2 local --L3 other $base $local $other -o $output
kdiff3.regkey=Software\KDiff3
kdiff3.regkeyalt=Software\Wow6432Node\KDiff3
kdiff3.regappend=\kdiff3.exe
kdiff3.fixeol=True
kdiff3.gui=True
kdiff3.diffargs=--L1 $plabel1 --L2 $clabel $parent $child

gvimdiff.args=--nofork -d -g -O $local $other $base
gvimdiff.regkey=Software\Vim\GVim
gvimdiff.regkeyalt=Software\Wow6432Node\Vim\GVim
gvimdiff.regname=path
gvimdiff.priority=-9
gvimdiff.diffargs=--nofork -d -g -O $parent $child

vimdiff.args=$local $other $base -c 'redraw | echomsg "hg merge conflict, type \":cq\" to abort vimdiff"'
vimdiff.check=changed
vimdiff.priority=-10

merge.check=conflicts
merge.priority=-100

gpyfm.gui=True

meld.gui=True
meld.args=--label='local' $local --label='merged' $base --label='other' $other -o $output
meld.check=changed
meld.diffargs=-a --label=$plabel1 $parent --label=$clabel $child

tkdiff.args=$local $other -a $base -o $output
tkdiff.gui=True
tkdiff.priority=-8
tkdiff.diffargs=-L $plabel1 $parent -L $clabel $child

xxdiff.args=--show-merged-pane --exit-with-merge-status --title1 local --title2 base --title3 other --merged-filename $output --merge $local $base $other
xxdiff.gui=True
xxdiff.priority=-8
xxdiff.diffargs=--title1 $plabel1 $parent --title2 $clabel $child

diffmerge.regkey=Software\SourceGear\SourceGear DiffMerge\
diffmerge.regkeyalt=Software\Wow6432Node\SourceGear\SourceGear DiffMerge\
diffmerge.regname=Location
diffmerge.priority=-7
diffmerge.args=-nosplash -merge -title1=local -title2=merged -title3=other $local $base $other -result=$output
diffmerge.check=changed
diffmerge.gui=True
diffmerge.diffargs=--nosplash --title1=$plabel1 --title2=$clabel $parent $child

p4merge.args=$base $local $other $output
p4merge.regkey=Software\Perforce\Environment
p4merge.regkeyalt=Software\Wow6432Node\Perforce\Environment
p4merge.regname=P4INSTROOT
p4merge.regappend=\p4merge.exe
p4merge.gui=True
p4merge.priority=-8
p4merge.diffargs=$parent $child

p4mergeosx.executable = /Applications/p4merge.app/Contents/MacOS/p4merge
p4mergeosx.args = $base $local $other $output
p4mergeosx.gui = True
p4mergeosx.priority=-8
p4mergeosx.diffargs=$parent $child

tortoisemerge.args=/base:$base /mine:$local /theirs:$other /merged:$output
tortoisemerge.regkey=Software\TortoiseSVN
tortoisemerge.regkeyalt=Software\Wow6432Node\TortoiseSVN
tortoisemerge.check=changed
tortoisemerge.gui=True
tortoisemerge.priority=-8
tortoisemerge.diffargs=/base:$parent /mine:$child /basename:$plabel1 /minename:$clabel

ecmerge.args=$base $local $other --mode=merge3 --title0=base --title1=local --title2=other --to=$output
ecmerge.regkey=Software\Elli\xc3\xa9 Computing\Merge
ecmerge.regkeyalt=Software\Wow6432Node\Elli\xc3\xa9 Computing\Merge
ecmerge.gui=True
ecmerge.diffargs=$parent $child --mode=diff2 --title1=$plabel1 --title2=$clabel

# editmerge is a small script shipped in contrib.
# It needs this config otherwise it behaves the same as internal:local
editmerge.args=$output
editmerge.check=changed
editmerge.premerge=keep

filemerge.executable=/Developer/Applications/Utilities/FileMerge.app/Contents/MacOS/FileMerge
filemerge.args=-left $other -right $local -ancestor $base -merge $output
filemerge.gui=True

filemergexcode.executable=/Applications/Xcode.app/Contents/Applications/FileMerge.app/Contents/MacOS/FileMerge
filemergexcode.args=-left $other -right $local -ancestor $base -merge $output
filemergexcode.gui=True

; Windows version of Beyond Compare
beyondcompare3.args=$local $other $base $output /ro /lefttitle=local /centertitle=base /righttitle=other /automerge /reviewconflicts /solo
beyondcompare3.regkey=Software\Scooter Software\Beyond Compare 3
beyondcompare3.regname=ExePath
beyondcompare3.gui=True
beyondcompare3.priority=-2
beyondcompare3.diffargs=/lro /lefttitle=$plabel1 /righttitle=$clabel /solo /expandall $parent $child

; Linux version of Beyond Compare
bcompare.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcompare.gui=True
bcompare.priority=-1
bcompare.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

; OS X version of Beyond Compare
bcomposx.executable = /Applications/Beyond Compare.app/Contents/MacOS/bcomp
bcomposx.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcomposx.gui=True
bcomposx.priority=-1
bcomposx.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

winmerge.args=/e /x /wl /ub /dl other /dr local $other $local $output
winmerge.regkey=Software\Thingamahoochie\WinMerge
winmerge.regkeyalt=Software\Wow6432Node\Thingamahoochie\WinMerge\
winmerge.regname=Executable
winmerge.check=changed
winmerge.gui=True
winmerge.priority=-10
winmerge.diffargs=/r /e /x /ub /wl /dl $plabel1 /dr $clabel $parent $child

araxis.regkey=SOFTWARE\Classes\TypeLib\{46799e0a-7bd1-4330-911c-9660bb964ea2}\7.0\HELPDIR
araxis.regappend=\ConsoleCompare.exe
araxis.priority=-2
araxis.args=/3 /a2 /wait /merge /title1:"Other" /title2:"Base" /title3:"Local :"$local $other $base $local $output
araxis.checkconflict=True
araxis.binary=True
araxis.gui=True
araxis.diffargs=/2 /wait /title1:$plabel1 /title2:$clabel $parent $child

diffuse.priority=-3
diffuse.args=$local $base $other
diffuse.gui=True
diffuse.diffargs=$parent $child

UltraCompare.regkey=Software\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.regkeyalt=Software\Wow6432Node\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.args = $base $local $other -title1 base -title3 other
UltraCompare.priority = -2
UltraCompare.gui = True
UltraCompare.binary = True
UltraCompare.check = conflicts,changed
UltraCompare.diffargs=$child $parent -title1 $clabel -title2 $plabel1
"#;

// Config items from python's configitems.py which previously were
// only available in Python. They have the lowest priority.
static HG_PY_CORE_CONFIG: &str = r#"
[treestate]
mingcage=900
minrepackthreshold=10M
repackfactor=3

[ui]
timeout=600

[checkout]
resumable=True

[tracing]
stderr=False
threshold=10
"#;
