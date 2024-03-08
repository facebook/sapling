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
use std::sync::Arc;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use hgplain;
use identity::Identity;
use minibytes::Text;
use url::Url;

use crate::config::ConfigSet;
use crate::config::Options;
use crate::error::Error;
use crate::error::Errors;
#[cfg(feature = "fb")]
use crate::fb::FbConfigMode;

pub trait OptionsHgExt {
    /// Drop configs according to `$HGPLAIN` and `$HGPLAINEXCEPT`.
    fn process_hgplain(self) -> Self;

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
    fn load(&mut self, repo_path: Option<&Path>) -> Result<(), Errors>;

    /// Load system config files if config environment variable is not set.
    /// Return errors parsing files.
    fn load_system(&mut self, opts: Options, identity: &Identity) -> Vec<Error>;

    /// Load the dynamic config files for the given repo path.
    /// Returns errors parsing, generating, or fetching the configs.
    fn load_dynamic(
        &mut self,
        repo_path: Option<&Path>,
        opts: Options,
        identity: &Identity,
        proxy_sock_path: Option<String>,
    ) -> Result<Vec<Error>>;

    /// Optionally refresh the dynamic config in the background.
    fn maybe_refresh_dynamic(&self, repo_path: Option<&Path>, identity: &Identity) -> Result<()>;

    /// Load user config files (and environment variables).  If config environment variable is
    /// set, load files listed in that environment variable instead.
    /// Return errors parsing files.
    fn load_user(&mut self, opts: Options, identity: &Identity) -> Vec<Error>;

    /// Load repo config files.
    fn load_repo(&mut self, repo_path: &Path, opts: Options, identity: &Identity) -> Vec<Error>;

    /// Load a specified config file. Respect HGPLAIN environment variables.
    /// Return errors parsing files.
    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error>;

    fn validate_dynamic(&mut self) -> Result<(), Error>;
}

/// Load config from specified repo root path, or global config if no path specified.
/// `extra_values` contains config overrides (i.e. "--config" CLI values).
/// `extra_files` contains additional config files (i.e. "--configfile" CLI values).
pub fn load(repo_path: Option<&Path>, pinned: &[PinnedConfig]) -> Result<ConfigSet> {
    let mut cfg = ConfigSet::new();
    let mut errors = Vec::new();

    tracing::debug!(?pinned, ?repo_path);

    // "--configfile" and "--config" values are loaded as "pinned". This lets us load them
    // first so they can inform further config loading, but also make sure they still take
    // precedence over "regular" configs.
    set_pinned_with_errors(&mut cfg, pinned, &mut errors);

    match cfg.load(repo_path) {
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

    Ok(cfg)
}

pub fn set_pinned(cfg: &mut ConfigSet, pinned: &[PinnedConfig]) -> Result<()> {
    let mut errors = Vec::new();
    set_pinned_with_errors(cfg, pinned, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Errors(errors).into())
    }
}

fn set_pinned_with_errors(cfg: &mut ConfigSet, pinned: &[PinnedConfig], errors: &mut Vec<Error>) {
    for pinned in pinned {
        let opts = Options::default().pin(true);

        match pinned {
            PinnedConfig::Raw(raw, source) => {
                if let Err(err) = set_override(cfg, raw, opts.clone().source(source.clone())) {
                    errors.push(err);
                }
            }
            PinnedConfig::KeyValue(section, name, value, source) => cfg.set(
                section,
                name,
                Some(value),
                &opts.clone().source(source.clone()),
            ),
            PinnedConfig::File(path, source) => {
                errors.extend(cfg.load_path(path.as_ref(), &opts.clone().source(source.clone())));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum PinnedConfig {
    // ("foo.bar=baz", <source>)
    Raw(Text, Text),
    // ("foo", "bar", "baz", <source>)
    KeyValue(Text, Text, Text, Text),
    // ("some/file.rc", <source>)
    File(Text, Text),
}

impl PinnedConfig {
    pub fn from_cli_opts(config: &[String], configfile: &[String]) -> Vec<Self> {
        // "--config" comes last so they take precedence
        configfile
            .iter()
            .map(|f| PinnedConfig::File(f.to_string().into(), "--configfile".into()))
            .chain(
                config
                    .iter()
                    .map(|c| PinnedConfig::Raw(c.to_string().into(), "--config".into())),
            )
            .collect()
    }
}

impl OptionsHgExt for Options {
    fn process_hgplain(self) -> Self {
        if hgplain::is_plain(None) {
            let (section_exclude_list, ui_exclude_list) = {
                let plain_exceptions = hgplain::exceptions();

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
}

/// override config values from a list of --config overrides
fn set_override(config: &mut ConfigSet, raw: &Text, opts: Options) -> crate::Result<()> {
    let equals_pos = raw
        .as_ref()
        .find('=')
        .ok_or_else(|| Error::ParseFlag(raw.to_string()))?;
    let section_name_pair = &raw[..equals_pos];
    let value = &raw[equals_pos + 1..];

    let dot_pos = section_name_pair
        .find('.')
        .ok_or_else(|| Error::ParseFlag(raw.to_string()))?;
    let section = &section_name_pair[..dot_pos];
    let name = &section_name_pair[dot_pos + 1..];

    config.set(section, name, Some(value), &opts);

    Ok(())
}

impl ConfigSetHgExt for ConfigSet {
    /// Load system, user config files.
    fn load(&mut self, repo_path: Option<&Path>) -> Result<(), Errors> {
        tracing::info!(
            repo_path = %repo_path.and_then(|p| p.to_str()).unwrap_or("<none>"),
            "loading config"
        );

        self.clear_unpinned();

        let ident = repo_path
            .map(|p| identity::must_sniff_dir(p).map_err(|e| Errors(vec![Error::Other(e)])))
            .transpose()?;
        let ident = ident.unwrap_or_else(identity::default);

        let repo_path = repo_path.map(|p| p.join(ident.dot_dir()));

        let mut errors = vec![];

        // Don't pin any configs we load. We are doing the "default" config loading should
        // be cleared if we load() again (via clear_unpinned());
        let opts = Options::new().pin(false);

        // The config priority from low to high is:
        //
        //   builtin
        //   dynamic
        //   system
        //   user
        //   repo
        //
        // We load things out of order a bit since the dynamic config can depend
        // on system config (namely, auth_proxy.unix_socket_path).

        // Clone rather than Self::new() so we include any --config overrides
        // already inside self.
        let mut dynamic = self.clone();

        errors.append(&mut self.load_system(opts.clone(), &ident));
        errors.append(&mut self.load_user(opts.clone(), &ident));

        // This is the out-of-orderness. We load the dynamic config on a
        // detached ConfigSet then combine it into our "secondary" config
        // sources to maintain the correct priority.
        errors.append(
            &mut dynamic
                .load_dynamic(
                    repo_path.as_deref(),
                    opts.clone(),
                    &ident,
                    self.get_opt("auth_proxy", "unix_socket_path")
                        .unwrap_or_default(),
                )
                .map_err(|e| Errors(vec![Error::Other(e)]))?,
        );

        let mut low_prio_configs = crate::builtin_static::builtin_system(opts.clone(), &ident);
        low_prio_configs.push(Arc::new(dynamic));
        self.secondary(Arc::new(low_prio_configs));

        if let Some(repo_path) = repo_path.as_deref() {
            errors.append(&mut self.load_repo(repo_path, opts, &ident));
            if let Err(e) = read_set_repo_name(self, repo_path) {
                errors.push(e);
            }
        }

        // Wait until config is fully loaded so maybe_refresh_dynamic() itself sees
        // correct config values.
        self.maybe_refresh_dynamic(repo_path.as_deref(), &ident)
            .map_err(|e| Errors(vec![Error::Other(e)]))?;

        if !errors.is_empty() {
            return Err(Errors(errors));
        }

        self.validate_dynamic().map_err(|err| Errors(vec![err]))
    }

    fn load_system(&mut self, opts: Options, ident: &Identity) -> Vec<Error> {
        let opts = opts.source("system").process_hgplain();
        let mut errors = Vec::new();

        for system_path in ident.system_config_paths() {
            if system_path.exists() {
                errors.append(&mut self.load_path(system_path, &opts));
            }
        }

        errors
    }

    #[cfg(feature = "fb")]
    fn load_dynamic(
        &mut self,
        repo_path: Option<&Path>,
        opts: Options,
        identity: &Identity,
        proxy_sock_path: Option<String>,
    ) -> Result<Vec<Error>> {
        use crate::fb::internalconfig::vpnless_config_path;

        let mut errors = Vec::new();
        let mode = FbConfigMode::from_identity(identity);

        tracing::debug!("FbConfigMode is {:?}", &mode);

        if !mode.need_dynamic_generator() {
            return Ok(errors);
        }

        // Compute path
        let dynamic_path = get_config_dir(repo_path)?.join("hgrc.dynamic");

        // Check version
        let content = read_to_string(&dynamic_path).ok();
        let version = content.as_ref().and_then(|c| {
            let mut lines = c.split('\n');
            match lines.next() {
                Some(line) if line.starts_with("# version=") => Some(&line[10..]),
                Some(_) | None => None,
            }
        });

        let this_version = ::version::VERSION;

        let vpnless_changed = match (dynamic_path.metadata(), vpnless_config_path().metadata()) {
            (Ok(d), Ok(v)) => v.modified()? > d.modified()?,
            _ => false,
        };

        // Synchronously generate the new config if it's out of date with our version
        if version != Some(this_version) || vpnless_changed {
            tracing::info!(?dynamic_path, file_version=?version, my_version=%this_version, vpnless_changed, "regenerating dynamic config (version mismatch)");
            let (repo_name, user_name) = {
                let mut temp_config = ConfigSet::new();
                if !temp_config.load_user(opts.clone(), identity).is_empty() {
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
            let res = generate_internalconfig(
                mode,
                repo_path,
                repo_name,
                None,
                user_name,
                proxy_sock_path,
            );
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
        } else {
            tracing::debug!(?dynamic_path, version=%this_version, "internalconfig version in-sync");
        }

        if !dynamic_path.exists() {
            return Err(IOError::new(
                ErrorKind::NotFound,
                format!("required config not found at {}", dynamic_path.display()),
            )
            .into());
        }

        // Read hgrc.dynamic
        let opts = opts.source("dynamic").process_hgplain();
        errors.append(&mut self.load_path(&dynamic_path, &opts));

        // Log config ages
        // - Done in python for now

        Ok(errors)
    }

    #[cfg(feature = "fb")]
    fn maybe_refresh_dynamic(&self, repo_path: Option<&Path>, identity: &Identity) -> Result<()> {
        use std::process::Command;
        use std::time::Duration;
        use std::time::SystemTime;

        use spawn_ext::CommandExt;

        let mode = FbConfigMode::from_identity(identity);
        if !mode.need_dynamic_generator() {
            return Ok(());
        }

        let dynamic_path = get_config_dir(repo_path)?.join("hgrc.dynamic");

        // Regenerate if mtime is old.
        let generation_time: Option<u64> = self.get_opt("configs", "generationtime")?;
        let recursion_marker = env::var("HG_INTERNALCONFIG_IS_REFRESHING");
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
                            vec![
                                identity::cli_name().to_string(),
                                "debugrefreshconfig".to_string(),
                            ]
                        })?;
                    tracing::debug!(
                        "spawn {:?} because mtime({}) {:?} > generation_time {:?}",
                        &config_regen_command,
                        dynamic_path.display(),
                        mtime_age,
                        generation_time
                    );
                    if !config_regen_command.is_empty() {
                        let mut command = Command::new(&config_regen_command[0]);
                        command
                            .args(&config_regen_command[1..])
                            .env("HG_INTERNALCONFIG_IS_REFRESHING", "1");

                        if let Some(repo_path) = repo_path {
                            command.current_dir(repo_path);
                        }

                        let _ = command.spawn_detached();
                    }
                } else {
                    skip_reason = Some("mtime <= configs.generationtime");
                }
            } else {
                skip_reason = Some("configs.generationtime is not set");
            }
        } else {
            skip_reason = Some("HG_INTERNALCONFIG_IS_REFRESHING is set");
        }
        if let Some(reason) = skip_reason {
            tracing::debug!("skip spawning debugrefreshconfig because {}", reason);
        }

        Ok(())
    }

    #[cfg(not(feature = "fb"))]
    fn maybe_refresh_dynamic(&self, _repo_path: Option<&Path>, _identity: &Identity) -> Result<()> {
        Ok(())
    }

    #[cfg(not(feature = "fb"))]
    fn load_dynamic(
        &mut self,
        _repo_path: Option<&Path>,
        _opts: Options,
        _identity: &Identity,
        _proxy_sock_path: Option<String>,
    ) -> Result<Vec<Error>> {
        Ok(Vec::new())
    }

    fn load_user(&mut self, opts: Options, ident: &Identity) -> Vec<Error> {
        let path = ident.user_config_path();
        self.load_user_internal(path.as_ref(), opts)
    }

    fn load_repo(&mut self, repo_path: &Path, opts: Options, identity: &Identity) -> Vec<Error> {
        let mut errors = Vec::new();

        let opts = opts.source("repo").process_hgplain();

        let repo_config_path = repo_path.join(identity.config_repo_file());
        errors.append(&mut self.load_path(repo_config_path, &opts));

        errors
    }

    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error> {
        let opts = Options::new().source(source).process_hgplain();
        self.load_path(path, &opts)
    }

    #[cfg(feature = "fb")]
    fn validate_dynamic(&mut self) -> Result<(), Error> {
        let allowed_locations: Option<Vec<String>> =
            self.get_opt::<Vec<String>>("configs", "allowedlocations")?;
        let allowed_configs: Option<Vec<String>> =
            self.get_opt::<Vec<String>>("configs", "allowedconfigs")?;

        Ok(self.ensure_location_supersets(
            allowed_locations
                .as_ref()
                .map(|v| HashSet::from_iter(v.iter().map(|s| s.as_str()))),
            allowed_configs.as_ref().map(|v| {
                HashSet::from_iter(v.iter().map(|s| {
                    s.split_once('.')
                        .expect("allowed configs must contain dots")
                }))
            }),
        ))
    }

    #[cfg(not(feature = "fb"))]
    fn validate_dynamic(&mut self) -> Result<(), Error> {
        Ok(())
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
            name = repo_name_from_url(config, &path).unwrap_or_default();
            if name.is_empty() {
                tracing::warn!("repo name: no path.default reponame: {}", &path);
            }
            source = "paths.default";
        }
        if name.is_empty() {
            match read_repo_name_from_disk(repo_path) {
                Ok(s) => {
                    name = s;
                    source = "reponame file";
                }
                Err(e) => {
                    tracing::warn!("repo name: no reponame file: {:?}", &e);
                }
            };
        }
        (name, source)
    };

    if !repo_name.is_empty() {
        tracing::debug!("repo name: {:?} (from {})", &repo_name, source);
        if source != "reponame file" {
            let need_rewrite = match read_repo_name_from_disk(repo_path) {
                Ok(s) => s != repo_name,
                Err(_) => true,
            };
            if need_rewrite {
                let path = get_repo_name_path(repo_path);
                match fs::write(path, &repo_name) {
                    Ok(_) => tracing::debug!("repo name: written to reponame file"),
                    Err(e) => tracing::warn!("repo name: cannot write to reponame file: {:?}", e),
                }
            }
        }
        if source != "remotefilelog.reponame" {
            config.set(
                "remotefilelog",
                "reponame",
                Some(&repo_name),
                &Options::default().source(source).pin(false),
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

trait ConfigSetExtInternal {
    fn load_user_internal(&mut self, path: Option<&PathBuf>, opts: Options) -> Vec<Error>;
}

impl ConfigSetExtInternal for ConfigSet {
    // For easier testing.
    fn load_user_internal(&mut self, path: Option<&PathBuf>, opts: Options) -> Vec<Error> {
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
                if !editor.is_empty() {
                    self.set(
                        "ui",
                        "editor",
                        Some(editor),
                        &opts.clone().source(format!("${}", name)),
                    );
                    break;
                }
            }
        }

        // Convert $HGPROF to profiling.type
        if let Ok(profiling_type) = env::var("HGPROF") {
            self.set("profiling", "type", Some(profiling_type), &"$HGPROF".into());
        }

        let opts = opts.source("user").process_hgplain();

        if let Some(path) = path {
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

/// Using custom "schemes" from config, resolve given url.
pub fn resolve_custom_scheme(config: &dyn Config, url: Url) -> Result<Url> {
    if let Some(tmpl) = config.get_nonempty("schemes", url.scheme()) {
        let non_scheme = match url.as_str().split_once(':') {
            Some((_, after)) => after.trim_start_matches('/'),
            None => bail!("url {url} has no scheme"),
        };

        let resolved_url = if tmpl.contains("{1}") {
            tmpl.replace("{1}", non_scheme)
        } else {
            format!("{tmpl}{non_scheme}")
        };

        return Url::parse(&resolved_url)
            .with_context(|| format!("parsing resolved custom scheme URL {resolved_url}"));
    }

    Ok(url)
}

pub fn repo_name_from_url(config: &dyn Config, s: &str) -> Option<String> {
    // Use a base_url to support non-absolute urls.
    let base_url = Url::parse("file:///.").unwrap();
    let parse_opts = Url::options().base_url(Some(&base_url));
    match parse_opts.parse(s) {
        Ok(url) => {
            let url = resolve_custom_scheme(config, url).ok()?;

            tracing::trace!("parsed url {}: {:?}", s, url);
            match url.scheme() {
                "mononoke" => {
                    // In Mononoke URLs, the repo name is always the full path
                    // with slashes trimmed.
                    let path = url.path().trim_matches('/');
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                }
                _ => {
                    // Try the last segment in url path.
                    if let Some(last_segment) = url
                        .path_segments()
                        .and_then(|s| s.rev().find(|s| !s.is_empty()))
                    {
                        return Some(last_segment.to_string());
                    }
                    // Try path. `path_segment` can be `None` for URL like "test:reponame".
                    let path = url.path().trim_matches('/');
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                    // Try the hostname. ex. in "fb://fbsource", "fbsource" is a host not a path.
                    // Also see https://www.mercurial-scm.org/repo/hg/help/schemes
                    if let Some(host_str) = url.host_str() {
                        return Some(host_str.to_string());
                    }
                }
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
                let trimmed = raw.trim_end_matches('\n');
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
pub fn calculate_internalconfig(
    mode: FbConfigMode,
    config_dir: PathBuf,
    repo_name: Option<impl AsRef<str>>,
    canary: Option<String>,
    user_name: String,
    proxy_sock_path: Option<String>,
) -> Result<ConfigSet> {
    use crate::fb::internalconfig::Generator;
    Generator::new(mode, repo_name, config_dir, user_name, proxy_sock_path)?.execute(canary)
}

#[cfg(feature = "fb")]
pub fn generate_internalconfig(
    mode: FbConfigMode,
    repo_path: Option<&Path>,
    repo_name: Option<impl AsRef<str>>,
    canary: Option<String>,
    user_name: String,
    proxy_sock_path: Option<String>,
) -> Result<()> {
    use std::io::Write;

    use filetime::set_file_mtime;
    use filetime::FileTime;
    use tempfile::tempfile_in;

    tracing::debug!(
        repo_path = ?repo_path,
        canary = ?canary,
        "generate_internalconfig",
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
            "# Generated by `hg debugrefreshconfig` - DO NOT MODIFY\n",
        ),
        version,
        repo_name.as_ref().map_or("no_repo", |r| r.as_ref()),
        canary.as_ref(),
        &user_name,
    );

    let hgrc_path = config_dir.join("hgrc.dynamic");
    let global_config_dir = get_config_dir(None)?;

    let config = calculate_internalconfig(
        mode,
        global_config_dir,
        repo_name,
        canary,
        user_name,
        proxy_sock_path,
    )?;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Write;

    use once_cell::sync::Lazy;
    use tempfile::TempDir;
    use testutil::envs::lock_env;

    use super::*;

    static CONFIG_ENV_VAR: Lazy<&str> =
        Lazy::new(|| identity::default().env_name_static("CONFIG").unwrap());
    static HGPLAIN: Lazy<&str> =
        Lazy::new(|| identity::default().env_name_static("PLAIN").unwrap());
    static HGPLAINEXCEPT: Lazy<&str> =
        Lazy::new(|| identity::default().env_name_static("PLAINEXCEPT").unwrap());

    fn write_file(path: PathBuf, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_basic_hgplain() {
        let mut env = lock_env();

        env.set(*HGPLAIN, Some("1"));
        env.set(*HGPLAINEXCEPT, None);

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
    fn test_static_config_hgplain() {
        let mut env = lock_env();

        for id in identity::all() {
            env.set(id.env_name_static("PLAIN").unwrap(), None);
            env.set(id.env_name_static("PLAINEXCEPT").unwrap(), None);
        }

        env.set("TESTTMP", Some("1"));

        let cfg = load(None, &[]).unwrap();

        // Sanity that we have a test value from static config.
        assert_eq!(
            cfg.get("alias", "some-command"),
            Some("some-command --some-flag".into())
        );
        let sources = cfg.get_sources("alias", "some-command");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &"builtin:test_config");

        // With HGPLAIN=1, aliases should get dropped.
        env.set(*HGPLAIN, Some("1"));
        let cfg = load(None, &[]).unwrap();
        assert_eq!(cfg.get("alias", "some-command"), None);
    }

    #[test]
    fn test_hgplainexcept() {
        let mut env = lock_env();

        env.set(*HGPLAIN, None);
        env.set(*HGPLAINEXCEPT, Some("alias,revsetalias"));

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

        use hgplain::is_plain;

        for id in identity::all() {
            env.set(id.env_name_static("PLAIN").unwrap(), None);
            env.set(id.env_name_static("PLAINEXCEPT").unwrap(), None);
        }

        assert!(!is_plain(None));

        env.set(*HGPLAIN, Some("1"));
        assert!(is_plain(None));
        assert!(is_plain(Some("banana")));

        env.set(*HGPLAINEXCEPT, Some("dog,banana,tree"));
        assert!(!is_plain(Some("banana")));

        env.set(*HGPLAIN, None);
        assert!(!is_plain(Some("banana")));
    }

    #[test]
    fn test_config_path() {
        let mut env = lock_env();

        let dir = TempDir::with_prefix("test_config_path.").unwrap();

        write_file(dir.path().join("1.rc"), "[x]\na=1");
        write_file(dir.path().join("2.rc"), "[y]\nb=2");
        write_file(dir.path().join("user.rc"), "");

        let hgrcpath = &[
            dir.path().join("1.rc").display().to_string(),
            dir.path().join("2.rc").display().to_string(),
            format!("user={}", dir.path().join("user.rc").display()),
        ]
        .join(";");
        env.set(*CONFIG_ENV_VAR, Some(hgrcpath));

        let mut cfg = ConfigSet::new();

        let identity = identity::default();
        cfg.load_user(Options::new(), &identity);
        assert_eq!(cfg.get("x", "a"), None);

        let identity = identity::default();
        cfg.load_system(Options::new(), &identity);
        assert_eq!(cfg.get("x", "a"), Some("1".into()));
        assert_eq!(cfg.get("y", "b"), Some("2".into()));
    }

    #[test]
    fn test_load_user() {
        let _env = lock_env();

        let dir = TempDir::with_prefix("test_hgrcpath.").unwrap();
        let path = dir.path().join("1.rc");

        write_file(path.clone(), "[ui]\nmerge=x");

        let mut cfg = ConfigSet::new();
        cfg.load_user_internal(Some(&path), Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "x");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge", Some("foo"), &"system".into());
        cfg.set("ui", "merge:interactive", Some("foo"), &"system".into());
        cfg.load_user_internal(Some(&path), Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "x");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge:interactive", Some("foo"), &"system".into());
        write_file(path.clone(), "[ui]\nmerge=x\nmerge:interactive=y\n");
        cfg.load_user_internal(Some(&path), Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "x");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "y");

        let mut cfg = ConfigSet::new();
        cfg.set("ui", "merge", Some("a"), &"system".into());
        cfg.set("ui", "merge:interactive", Some("b"), &"system".into());
        write_file(path.clone(), "");
        cfg.load_user_internal(Some(&path), Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "a");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "b");
        write_file(path.clone(), "[ui]\nmerge:interactive=y\n");
        cfg.load_user_internal(Some(&path), Options::new());
        assert_eq!(cfg.get("ui", "merge").unwrap(), "a");
        assert_eq!(cfg.get("ui", "merge:interactive").unwrap(), "y");

        drop(path);
    }

    #[test]
    fn test_load_hgrc() {
        let dir = TempDir::with_prefix("test_hgrcpath.").unwrap();
        let path = dir.path().join("1.rc");

        write_file(path.clone(), "[x]\na=1\n[alias]\nb=c\n");

        let mut env = lock_env();

        for id in identity::all() {
            env.set(id.env_name_static("PLAIN").unwrap(), None);
            env.set(id.env_name_static("PLAINEXCEPT").unwrap(), None);
        }

        env.set(*HGPLAIN, Some("1"));
        env.set(*HGPLAINEXCEPT, None);

        let mut cfg = ConfigSet::new();
        cfg.load_hgrc(&path, "hgrc");

        assert!(cfg.keys("alias").is_empty());
        assert!(cfg.get("alias", "b").is_none());
        assert_eq!(cfg.get("x", "a").unwrap(), "1");

        env.set(*HGPLAIN, None);
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
    fn test_py_core_items() {
        let mut env = lock_env();

        // Skip real dynamic config.
        env.set("TESTTMP", Some("1"));

        let mut cfg = ConfigSet::new();
        cfg.load(None).unwrap();
        assert_eq!(cfg.get("treestate", "repackfactor").unwrap(), "3");
    }

    #[test]
    fn test_load_cli_args() {
        let mut env = lock_env();

        // Skip real dynamic config.
        env.set("TESTTMP", Some("1"));

        let dir = TempDir::with_prefix("test_load.").unwrap();

        let repo_rc = dir.path().join(".hg/hgrc");
        write_file(repo_rc, "[s]\na=orig\nb=orig\nc=orig");

        let other_rc = dir.path().join("other.rc");
        write_file(other_rc.clone(), "[s]\na=other\nb=other");

        let cfg = load(
            Some(dir.path()),
            &[
                PinnedConfig::File(
                    format!("{}", other_rc.display()).into(),
                    "--configfile".into(),
                ),
                PinnedConfig::Raw("s.b=flag".into(), "--config".into()),
            ],
        )
        .unwrap();

        assert_eq!(cfg.get("s", "a"), Some("other".into()));
        assert_eq!(cfg.get("s", "b"), Some("flag".into()));
        assert_eq!(cfg.get("s", "c"), Some("orig".into()));
    }

    #[test]
    fn test_repo_name_from_url() {
        let config = BTreeMap::<&str, &str>::from([("schemes.fb", "mononoke://example.com/{1}")]);

        let check = |url, name| {
            assert_eq!(repo_name_from_url(&config, url).as_deref(), name);
        };

        // Ordinary schemes use the basename as the repo name
        check("repo", Some("repo"));
        check("../path/to/repo", Some("repo"));
        check("file:repo", Some("repo"));
        check("file:/path/to/repo", Some("repo"));
        check("file://server/path/to/repo", Some("repo"));
        check("ssh://user@host/repo", Some("repo"));
        check("ssh://user@host/path/to/repo", Some("repo"));
        check("file:/", None);

        // This isn't correct, but is a side-effect of earlier hacks (should
        // be `None`)
        check("ssh://user@host:100/", Some("host"));

        // Mononoke scheme uses the full path, and repo names can contain
        // slashes.
        check("mononoke://example.com/repo", Some("repo"));
        check("mononoke://example.com/path/to/repo", Some("path/to/repo"));
        check("mononoke://example.com/", None);

        // FB scheme uses the full path.
        check("fb:repo", Some("repo"));
        check("fb:path/to/repo", Some("path/to/repo"));
        check("fb:", None);

        // FB scheme works even when there are extra slashes that shouldn't be
        // there.
        check("fb://repo/", Some("repo"));
        check("fb://path/to/repo", Some("path/to/repo"));
    }

    #[test]
    fn test_resolve_custom_scheme() {
        let config = BTreeMap::<&str, &str>::from([
            ("schemes.append", "appended://bar/"),
            ("schemes.subst", "substd://bar/{1}/baz"),
        ]);

        let check = |url, resolved| {
            assert_eq!(
                resolve_custom_scheme(&config, Url::parse(url).unwrap())
                    .unwrap()
                    .as_str(),
                resolved
            );
        };

        check("other://foo", "other://foo");
        check("append:one/two", "appended://bar/one/two");
        check("subst://one/two", "substd://bar/one/two/baz");
    }
}