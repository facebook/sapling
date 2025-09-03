/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::env::VarError;
use std::fs;
use std::fs::read_link;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use derivative::Derivative;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

pub mod dotgit;

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct Identity {
    user: &'static UserIdentity,
    repo: &'static RepoIdentity,
}

#[derive(PartialEq, Debug, Clone, Copy)]
struct UserIdentity {
    /// Name of the binary. Used for showing help messages
    ///
    /// Example: `Checkout failed. Resume with 'sl goto --continue'`
    cli_name: &'static str,

    /// Name of the product
    product_name: &'static str,

    /// Full name of the product
    long_product_name: &'static str,

    /// Prefix of environment variables related to current repo.
    env_prefix: &'static str,

    /// Subdirectory below user's config directory. The user's config directory depends on platform:
    ///
    /// |Platform | Value                                 | Example                                  |
    /// | ------- | ------------------------------------- | ---------------------------------------- |
    /// | Linux   | `$XDG_CONFIG_HOME` or `$HOME`/.config | /home/alice/.config/sapling              |
    /// | macOS   | `$HOME`/Library/Preferences           | /Users/Alice/Library/Preferences/sapling |
    /// | Windows | `{FOLDERID_RoamingAppData}`           | C:\Users\Alice\AppData\Roaming\sapling   |
    ///
    /// If None, config file is directly in home directory.
    config_user_directory: Option<&'static str>,

    /// User config file names to look for inside of `config_directory`.
    config_user_files: &'static [&'static str],

    /// System config file path, typically installed by package or system administrator.
    /// On Windows, `config_system_path` is a path fragment that gets appended to %PROGRAMDATA%.
    config_system_path: &'static str,

    /// Disables any configuration settings that might change the default output, including but not
    /// being limited to encoding, defaults, verbose mode, debug mode, quiet mode, and tracebacks
    ///
    /// See `<cli_name> help scripting`` for more details
    scripting_env_var: &'static str,

    /// If this environment variable is set, its value is considered the only file to look into for
    /// system and user configs
    scripting_config_env_var: &'static str,

    /// Comma-separated list of features to preserve if `scripting_env_var` is enabled
    scripting_except_env_var: &'static str,
}

#[derive(Derivative, Debug, Clone, Copy)]
#[derivative(PartialEq)]
struct RepoIdentity {
    /// Metadata directory of the current identity. If this directory exists in the current repo, it
    /// implies that the repo is using this identity.
    dot_dir: &'static str,

    /// Config file for the repo; located inside of `dot_dir`.
    ///
    /// Examples: `config`, `hgrc`
    config_repo_file: &'static str,

    /// Directory used by `sniff_dir`. Reuse `dot_dir` if `None`.
    sniff_dot_dir: Option<&'static str>,

    /// Files under the "sniff dot dir". They must exist to validate the sniff.
    sniff_dot_dir_required_files: &'static [&'static str],

    /// Affects `sniff_root`. Lower number wins.
    /// For example, `a/.sl` with priority 0 and `a/b/.git/sl` with priority 10,
    /// `a/.sl` wins even if it's not the inner-most directory.
    sniff_root_priority: usize,

    /// If set, the initial cli_name must be part of this value for "sniff" to work.
    /// This is useful to avoid potential risks that (potentially used in automation)
    /// that "hg root" succeeding in a `.git` repo.
    sniff_initial_cli_names: Option<&'static str>,

    /// Function. Turn (working_copy_root, dot_dir) to full_dot_dir.
    #[derivative(PartialEq = "ignore")]
    resolve_dot_dir_func: fn(&Path, &'static str) -> PathBuf,
}

impl Identity {
    pub fn cli_name(&self) -> &'static str {
        self.user.cli_name
    }

    pub fn product_name(&self) -> &'static str {
        self.user.product_name
    }

    pub fn long_product_name(&self) -> &'static str {
        self.user.long_product_name
    }

    /// Obtain the static ".hg" or ".sl", or ".git/sl" directory name.
    ///
    /// Note: In complex cases (ex. dotgit + submodule) the full dot_dir is not
    /// as simple as `repo_root.join(dot_dir)`. Use `resolve_dot_dir` instead.
    pub fn dot_dir(&self) -> &'static str {
        self.repo.dot_dir
    }

    /// Obtain the full ".hg" or ".sl", or ".git/sl" path, given the working
    /// copy root. This function handles complexity like dotgit and submodules.
    ///
    /// `root` is the "repo root" that matches `sl root` output.
    pub fn resolve_full_dot_dir(&self, root: &Path) -> PathBuf {
        (self.repo.resolve_dot_dir_func)(root, self.repo.dot_dir)
    }

    pub fn config_repo_file(&self) -> &'static str {
        self.repo.config_repo_file
    }

    pub fn env_prefix(&self) -> &'static str {
        self.user.env_prefix
    }

    pub const fn env_name_static(&self, suffix: &str) -> Option<&'static str> {
        // Use byte slice to workaround const_fn limitation.
        let bsuffix = suffix.as_bytes();
        match bsuffix {
            b"CONFIG" => Some(self.user.scripting_config_env_var),
            b"PLAIN" => Some(self.user.scripting_env_var),
            b"PLAINEXCEPT" => Some(self.user.scripting_except_env_var),
            _ => None,
        }
    }

    pub fn env_name(&self, suffix: &str) -> Cow<'static, str> {
        match self.env_name_static(suffix) {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned([self.user.env_prefix, suffix].concat()),
        }
    }

    pub fn env_var(&self, suffix: &str) -> Option<Result<String, VarError>> {
        let var_name = self.env_name(suffix);
        match std::env::var(var_name.as_ref()) {
            Err(VarError::NotPresent) => None,
            Err(err) => Some(Err(err)),
            Ok(val) => Some(Ok(val)),
        }
    }

    pub fn user_config_paths(&self) -> Vec<PathBuf> {
        // Read from "CONFIG" env var
        if let Some(Ok(rcpath)) = self.env_var("CONFIG") {
            let paths = split_rcpath(&rcpath, &["user"]);
            let paths: Vec<PathBuf> = paths
                .flat_map(|p| {
                    if p == "." {
                        self.all_builtin_user_config_paths()
                    } else {
                        vec![PathBuf::from(p)]
                    }
                })
                .collect();
            // paths.is_empty() test is for test compatibility.
            if !paths.is_empty() {
                tracing::debug!("user_config_paths from CONFIG: {:?}", paths);
                return paths;
            }
        }

        let paths = self.all_builtin_user_config_paths();
        tracing::debug!("user_config_paths from builtin: {:?}", paths);
        paths
    }

    fn all_builtin_user_config_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.builtin_user_config_paths();
        for ident in all() {
            if ident.cli_name() != self.cli_name() {
                paths.append(&mut ident.builtin_user_config_paths());
            }
        }
        paths
    }

    fn builtin_user_config_paths(&self) -> Vec<PathBuf> {
        let config_dir = match self.user.config_user_directory {
            None => match home_dir() {
                None => return Vec::new(),
                Some(hd) => hd,
            },
            Some(subdir) => {
                let config_dir = if cfg!(windows) {
                    std::env::var("APPDATA")
                        .map_or_else(|_| dirs::config_dir(), |x| Some(PathBuf::from(x)))
                } else if cfg!(target_os = "macos") {
                    // Argh! The `dirs` crate changed `config_dir()` on mac from "Preferences" to
                    // "Application Support". See https://github.com/dirs-dev/directories-rs/issues/62
                    // for discussion. I think Preferences is still a more suitable place for our
                    // user config, even if we aren't using Apple's preferences API to write out the
                    // file.
                    dirs::preference_dir()
                } else {
                    dirs::config_dir()
                };
                match config_dir {
                    None => return Vec::new(),
                    Some(config_dir) => config_dir.join(subdir),
                }
            }
        };

        let paths = self
            .user
            .config_user_files
            .iter()
            .map(|f| config_dir.join(f))
            .collect();
        paths
    }

    /// Return the first user config path that exists.
    /// If none of the paths exist, return the first user config path.
    /// Might return `None` if `CONFIG` does not specify user config paths.
    pub fn user_config_path(&self) -> Option<PathBuf> {
        let paths = self.user_config_paths();
        paths
            .iter()
            .find(|p| p.exists())
            .cloned()
            .or_else(|| paths.into_iter().next())
    }

    pub fn system_config_paths(&self) -> Vec<PathBuf> {
        // Read from "CONFIG" env var
        if let Some(Ok(rcpath)) = self.env_var("CONFIG") {
            let paths = split_rcpath(&rcpath, &["sys", ""]);
            let paths = paths
                .flat_map(|p| {
                    if p == "." {
                        self.all_builtin_system_config_paths()
                    } else {
                        vec![PathBuf::from(p)]
                    }
                })
                .collect();
            tracing::debug!("system_config_paths from CONFIG: {:?}", paths);
            return paths;
        }

        // Also include paths from other identities.
        let paths = self.all_builtin_system_config_paths();
        tracing::debug!("system_config_paths from builtin: {:?}", paths);
        paths
    }

    fn all_builtin_system_config_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.builtin_system_config_paths();
        for ident in all() {
            if ident.cli_name() != self.cli_name() {
                paths.append(&mut ident.builtin_system_config_paths());
            }
        }
        paths
    }

    fn builtin_system_config_paths(&self) -> Vec<PathBuf> {
        if cfg!(windows) {
            let mut result = Vec::new();
            if let Some(dir) = std::env::var_os("PROGRAMDATA") {
                result.push(PathBuf::from(dir).join(self.user.config_system_path))
            }
            result
        } else {
            vec![self.user.config_system_path.into()]
        }
    }

    pub fn punch(&self, tmpl: &str) -> String {
        tmpl.replace("@prog@", self.cli_name())
            .replace("@Product@", self.product_name())
    }

    pub fn is_dot_git(&self) -> bool {
        self.dot_dir() == SL_GIT.repo.dot_dir
    }
}

fn default_resolve_dot_dir_func(root: &Path, dot_dir: &'static str) -> PathBuf {
    root.join(dot_dir)
}

/// Split the HGRCPATH. Return items matching at least one of the given prefix.
///
/// `;` can be used as the separator on all platforms.
/// `:` can be used as the separator on non-Windows platforms.
pub fn split_rcpath<'a>(
    rcpath: &'a str,
    prefix_list: &'static [&'static str],
) -> impl Iterator<Item = &'a str> {
    const KNOWN_PREFIXES: &[&str] = if cfg!(feature = "fb") {
        &["sys", "user", "fb" /* See D48042830 */]
    } else {
        &["sys", "user"]
    };

    let sep = if cfg!(windows) {
        &[';'][..]
    } else {
        &[';', ':'][..]
    };
    let paths = rcpath.split(sep);
    paths.filter_map(|path| {
        tracing::trace!("RCPATH component: {}", path);
        let mut split = path.splitn(2, '=');
        if let Some(prefix) = split.next() {
            if prefix_list.contains(&prefix) {
                return split.next();
            } else if KNOWN_PREFIXES.contains(&prefix) {
                return None;
            };
        }
        // Unknown prefix.
        if prefix_list.contains(&"") {
            Some(path)
        } else {
            None
        }
    })
}

fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        // dirs::home_dir doesn't respect USERPROFILE. Check it for
        // compatibility with tests.
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            return Some(user_profile.into());
        }
    }

    dirs::home_dir()
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user.cli_name)
    }
}

const HG: Identity = Identity {
    user: &UserIdentity {
        cli_name: "hg",
        product_name: "Sapling",
        long_product_name: "Sapling SCM",
        env_prefix: "HG",
        config_user_directory: None,
        config_user_files: &[
            ".hgrc",
            #[cfg(windows)]
            "mercurial.ini",
        ],
        #[cfg(windows)]
        config_system_path: r"Facebook\Mercurial\system.rc",
        #[cfg(not(windows))]
        config_system_path: "/etc/mercurial/system.rc",
        scripting_env_var: "HGPLAIN",
        scripting_config_env_var: "HGRCPATH",
        scripting_except_env_var: "HGPLAINEXCEPT",
    },

    repo: &RepoIdentity {
        dot_dir: ".hg",
        config_repo_file: "hgrc",
        sniff_dot_dir: None,
        sniff_dot_dir_required_files: &["requires"],
        sniff_root_priority: 0,
        sniff_initial_cli_names: None,
        resolve_dot_dir_func: default_resolve_dot_dir_func,
    },
};

const SL: Identity = Identity {
    user: &UserIdentity {
        cli_name: "sl",
        product_name: "Sapling",
        long_product_name: "Sapling SCM",
        env_prefix: "SL_",
        config_user_directory: Some("sapling"),
        config_user_files: &["sapling.conf"],
        #[cfg(windows)]
        config_system_path: r"Sapling\system.conf",
        #[cfg(not(windows))]
        config_system_path: "/etc/sapling/system.conf",
        scripting_env_var: "SL_AUTOMATION",
        scripting_config_env_var: "SL_CONFIG_PATH",
        scripting_except_env_var: "SL_AUTOMATION_EXCEPT",
    },

    repo: &RepoIdentity {
        dot_dir: ".sl",
        config_repo_file: "config",
        sniff_dot_dir: None,
        sniff_dot_dir_required_files: &["requires"],
        sniff_root_priority: 0,
        sniff_initial_cli_names: None,
        resolve_dot_dir_func: default_resolve_dot_dir_func,
    },
};

/// `.git/` compatibility mode; scalability is limited by Git.
const SL_GIT: Identity = Identity {
    repo: &RepoIdentity {
        dot_dir: if cfg!(windows) { ".git\\sl" } else { ".git/sl" },
        sniff_dot_dir: Some(".git"),
        sniff_dot_dir_required_files: &[],
        sniff_root_priority: 10, // lowest
        sniff_initial_cli_names: Some("sl"),
        resolve_dot_dir_func: dotgit::resolve_dot_dir_func,
        ..*SL.repo
    },
    ..SL
};

#[cfg(test)]
const TEST: Identity = Identity {
    user: &UserIdentity {
        cli_name: "test",
        product_name: "Test",
        long_product_name: "Testing SCM",
        env_prefix: "TEST",
        config_user_directory: None,
        config_user_files: &["test.conf"],
        config_system_path: "test",
        scripting_env_var: "TEST_SCRIPT",
        scripting_config_env_var: "TEST_RC_PATH",
        scripting_except_env_var: "TEST_SCRIPT_EXCEPT",
    },

    repo: &RepoIdentity {
        dot_dir: ".test",
        config_repo_file: "config",
        sniff_dot_dir: None,
        sniff_dot_dir_required_files: &[],
        sniff_root_priority: 5,
        sniff_initial_cli_names: None,
        resolve_dot_dir_func: default_resolve_dot_dir_func,
    },
};

#[cfg(all(not(feature = "sl_oss"), not(test)))]
mod idents {
    use super::*;

    pub fn all() -> &'static [Identity] {
        &[SL, HG]
    }
}

#[cfg(feature = "sl_oss")]
mod idents {
    use super::*;

    pub fn all() -> &'static [Identity] {
        if in_test() { &[SL, HG] } else { &[SL] }
    }
}

#[cfg(test)]
pub mod idents {
    use super::*;

    pub fn all() -> &'static [Identity] {
        &[HG, SL, TEST]
    }
}

static EXTRA_SNIFF_IDENTS: &[Identity] = &[SL_GIT];

static DEFAULT: Lazy<RwLock<Identity>> = Lazy::new(|| RwLock::new(compute_default()));

pub use idents::all;

pub fn default() -> Identity {
    *DEFAULT.read()
}

pub fn reset_default() {
    // Cannot use INITIAL_DEFAULT - env vars might have changed.
    *DEFAULT.write() = compute_default();
}

/// Default `Identity` based on the current executable name.
fn compute_default() -> Identity {
    let path = std::env::current_exe().expect("current_exe() should not fail");
    let file_name = path
        .file_name()
        .expect("file_name() on current_exe() should not fail");
    let file_name = file_name.to_string_lossy();
    let (mut ident, reason) = (|| {
        // Allow overriding identity selection via env var (e.g. "SL_IDENTITY=sl").

        if let Some(env_override) = env_var_any("IDENTITY").and_then(|v| v.ok()) {
            for ident in all() {
                if ident.user.cli_name == env_override {
                    tracing::debug!(ident = env_override, "override ident from env");
                    return (*ident, "env var");
                }
            }
        }

        for ident in all() {
            if file_name.contains(ident.user.cli_name) {
                return (*ident, "contains");
            }
        }

        // Fallback to SL if current_exe does not provide information.
        (SL, "fallback")
    })();

    // Allow overriding the repo identity when creating repo (i.e. choose flavor of dot dir).
    // When repo already exists, repo identity will always be based on existing dot dir.
    if let Some(env_repo_override) = env_var_any("REPO_IDENTITY").and_then(|v| v.ok()) {
        if let Some(repo_ident) = all()
            .iter()
            .find(|id| id.user.cli_name == env_repo_override)
        {
            tracing::debug!(
                repo_ident = repo_ident.dot_dir(),
                "override repo ident from env"
            );
            ident.repo = repo_ident.repo;
        }
    }

    tracing::info!(
        identity = ident.user.cli_name,
        argv0 = file_name.as_ref(),
        reason,
        "identity from argv0"
    );

    ident
}

impl RepoIdentity {
    fn sniff_dot_dir(&self) -> &'static str {
        self.sniff_dot_dir.unwrap_or(self.dot_dir)
    }
}

/// CLI name to be used in user facing messaging.
pub fn cli_name() -> &'static str {
    DEFAULT.read().cli_name()
}

/// Sniff the given path for the existence of "{path}/.hg" or
/// "{path}/.sl" directories, yielding the sniffed Identity, if any.
/// Only permissions errors are propagated.
pub fn sniff_dir(path: &Path) -> Result<Option<Identity>> {
    'outer_loop: for id in all().iter().chain(EXTRA_SNIFF_IDENTS) {
        if let Some(cli_names) = id.repo.sniff_initial_cli_names {
            // Support bypassing the CLI name check via PLAINEXCEPT=sniff. This can be useful for ISL.
            let mut bypass_check = false;
            if let Ok(except) = try_env_var("PLAINEXCEPT") {
                if except.contains("sniff") {
                    bypass_check = true;
                }
            }
            if !bypass_check && !cli_names.contains(cli_name()) {
                continue;
            }
        }
        let sniff_dot_dir = id.repo.sniff_dot_dir();
        let test_path = path.join(sniff_dot_dir);
        tracing::trace!(path=%path.display(), "sniffing dir");
        match fs::metadata(&test_path) {
            Ok(md)
                if md.is_dir()
                    || (md.is_file()
                        && sniff_dot_dir == ".git"
                        && fs::read(&test_path)
                            .unwrap_or_default()
                            .starts_with(b"gitdir: ")) =>
            {
                // Check sniff_dot_dir_required_files.
                // This does not follow ".git" as a file yet.
                if md.is_dir() {
                    for path in id.repo.sniff_dot_dir_required_files {
                        let path = test_path.join(path);
                        if matches!(path.try_exists(), Ok(false)) {
                            // Reject this as a dotdir.
                            continue 'outer_loop;
                        }
                    }
                }
                tracing::debug!(id=%id, path=%path.display(), "sniffed repo dir");

                // Combine DEFAULT's user facing attributes w/ id's repo attributes.
                let mut mix = *DEFAULT.read();
                mix.repo = id.repo;

                return Ok(Some(mix));
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                // Propagate permission error checking dot dir so we
                // don't infer the wrong identity. Ideally this would
                // be an allowlist of errors, but unstable errors like
                // NotADirectory are unmatchable for now.
                return Err::<_, Error>(err.into()).with_context(|| {
                    format!("error sniffing {} for identity", test_path.display())
                });
            }
            _ => {}
        };
    }

    Ok(None)
}

/// Like sniff_dir, but returns an error instead of None.
pub fn must_sniff_dir(path: &Path) -> Result<Identity> {
    sniff_dir(path)?.with_context(|| format!("repo {} missing dot dir", path.display()))
}

/// Recursively sniff path and its ancestors for the first directory
/// containing a ".hg" or ".sl" directory. The ancestor directory and
/// corresponding Identity are returned, if any. Only permission
/// errors are propagated.
///
/// This function does not write to the filesystem. It does not auto create
/// `.git/sl`, despite still returns `SL_GIT` identity for `.git`. `clidispatch`
/// calls `gitcompat::init::maybe_init_inside_dotgit` to create `.git/sl` on
/// repo construction.
pub fn sniff_root(path: &Path) -> Result<Option<(PathBuf, Identity)>> {
    tracing::debug!(start=%path.display(), "sniffing for repo root");

    let mut path = Some(path.to_path_buf());
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // We first check all of `path` without following symlinks to maintain existing
    // behavior when operating on symlinks within the repo. We then retry, following the
    // deepest symlink we encountered in `path`. 0..2 gives two iterations which means we
    // will follow at most one symlink. We can raise this if needed, but it seemed prudent
    // to limit the scope to "reasonable" symlink situations.
    for _ in 0..2 {
        let mut best_priority = usize::MAX;
        let mut best = None;
        let mut final_symlink = None;

        while let Some(p) = &path {
            if !seen.insert(p.to_path_buf()) {
                break;
            }

            if let Some(ident) = sniff_dir(p)? {
                if ident.repo.sniff_root_priority == 0 {
                    return Ok(Some((p.to_path_buf(), ident)));
                } else if best_priority > ident.repo.sniff_root_priority {
                    best_priority = ident.repo.sniff_root_priority;
                    best = Some((p.to_path_buf(), ident));
                }
            }

            if final_symlink.is_none() && p.is_symlink() {
                final_symlink = Some(p.to_path_buf());
            }

            path = p.parent().map(|p| p.to_path_buf());
        }

        if best.is_some() {
            return Ok(best);
        }

        // We didn't find a repo - try following the final symlink we saw.
        if let Some(symlink) = final_symlink {
            if let Ok(mut target) = read_link(&symlink) {
                // Resolve relative symlink.
                if let Some(link_parent) = symlink.parent() {
                    target = link_parent.join(target);
                }
                path = Some(target)
            }
        }
    }

    Ok(None)
}

/// Recursively call `sniff_root` to get all the repo directories
/// and corresponding identities up to system root `/`.
/// Returns an empty vector if no valid repository is found.
/// Note that it still respects sniff_root_priority, so it's possible that
/// some roots of low priority get skipped when those of high priority exist.
pub fn sniff_roots(path: &Path) -> Result<Vec<(PathBuf, Identity)>> {
    let mut roots: Vec<(PathBuf, Identity)> = Vec::new();
    let mut seen = HashSet::new();
    let mut first_ident = None;

    let mut curr_path = Some(path.to_path_buf());
    while let Some(p) = curr_path.take() {
        if !seen.insert(p.to_path_buf()) {
            break;
        }
        if let Some((root, ident)) = sniff_root(&p)? {
            // Various repo identities usually indicate errors,
            // since in general we don't support nested repos.
            if first_ident.is_none() {
                first_ident = Some(ident);
            } else if ident != first_ident.unwrap() {
                return Err(anyhow::anyhow!(
                    "Various repo identities ({} and {}) found, which indicates an error.\n\
                    Sapling does not support nested repos of different kinds.",
                    first_ident.unwrap().repo.sniff_dot_dir(),
                    ident.repo.sniff_dot_dir()
                ));
            }

            roots.push((root.to_path_buf(), ident));
            curr_path = root.parent().map(|p| p.to_path_buf());
        }
    }

    Ok(roots)
}

pub fn env_var(var_suffix: &str) -> Option<Result<String, VarError>> {
    let current_id = DEFAULT.read();

    // Always prefer current identity.
    if let Some(res) = current_id.env_var(var_suffix) {
        return Some(res);
    }

    // Backwards compat for old env vars.
    env_var_any(var_suffix)
}

fn env_var_any(var_suffix: &str) -> Option<Result<String, VarError>> {
    for id in all() {
        if let Some(res) = id.env_var(var_suffix) {
            return Some(res);
        }
    }
    None
}

pub fn try_env_var(var_suffix: &str) -> Result<String, VarError> {
    match env_var(var_suffix) {
        Some(result) => result,
        None => Err(VarError::NotPresent),
    }
}

pub fn debug_env_var(name: &str) -> Option<(String, String)> {
    for maybe_name in [format!("SL_{name}"), format!("EDENSCM_{name}")] {
        if let Ok(val) = std::env::var(&maybe_name) {
            return Some((maybe_name, val));
        }
    }

    if name == "LOG" && in_test() {
        std::env::var(name).ok().map(|v| (name.to_string(), v))
    } else {
        None
    }
}

fn in_test() -> bool {
    std::env::var("TESTTMP").is_ok()
}

#[cfg(test)]
mod test {
    use std::fs;

    use super::*;

    #[test]
    fn test_sniff_dir() -> Result<()> {
        let dir = tempfile::tempdir()?;

        assert!(sniff_dir(&dir.path().join("doesn't exist"))?.is_none());

        {
            let root = dir.path().join("default");
            fs::create_dir_all(root.join(default().dot_dir()))?;
            write_required_files(&root, default());

            assert_eq!(sniff_dir(&root)?.unwrap(), default());
        }

        {
            let root = dir.path().join("test1");
            fs::create_dir_all(root.join(TEST.dot_dir()))?;

            let sniffed = sniff_dir(&root)?.unwrap();
            assert_eq!(sniffed.repo, TEST.repo);
            assert_eq!(sniffed.user, default().user);
        }

        // Make sure we don't error out on bundle file (e.g. "hg -R some_bundle ...").
        {
            let bundle = dir.path().join("foo/bundle.hg");
            fs::create_dir_all(bundle.parent().unwrap())?;
            let _ = fs::File::create(&bundle).unwrap();
            assert!(sniff_dir(&bundle)?.is_none());
        }

        #[cfg(unix)]
        {
            let root = dir.path().join("bad_perms");
            let dot_dir = root.join(default().dot_dir());
            fs::create_dir_all(dot_dir)?;

            // Sanity.
            assert!(sniff_dir(&root).is_ok());

            let perm = std::os::unix::fs::PermissionsExt::from_mode(0o0);
            fs::File::open(&root)?.set_permissions(perm)?;

            // Make sure we error out if we can't read the dot dir.
            assert!(sniff_dir(&root).is_err());
        }

        Ok(())
    }

    #[test]
    fn test_sniff_root() -> Result<()> {
        let dir = tempfile::tempdir()?;

        let root = dir.path().join("root");

        assert_eq!(sniff_root(&root)?, None);

        let dot_dir = root.join(TEST.dot_dir());
        fs::create_dir_all(dot_dir)?;

        let (sniffed_root, sniffed_ident) = sniff_root(&root)?.unwrap();
        assert_eq!(sniffed_root, root);
        assert_eq!(sniffed_ident.repo, TEST.repo);
        assert_eq!(sniffed_ident.user, default().user);

        let abc = root.join("a/b/c");
        fs::create_dir_all(abc)?;

        let (sniffed_root, sniffed_ident) = sniff_root(&root)?.unwrap();
        assert_eq!(sniffed_root, root);
        assert_eq!(sniffed_ident.repo, TEST.repo);
        assert_eq!(sniffed_ident.user, default().user);

        Ok(())
    }

    #[test]
    fn test_sniff_roots_in_nested_repos() -> Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(sniff_roots(dir.path())?.len(), 0);

        let dot_dir = dir.path().join(TEST.dot_dir());
        fs::create_dir_all(dot_dir)?;

        let a = dir.path().join("a");
        let ab = a.join("b");
        let abc = ab.join("c");
        fs::create_dir_all(&a)?;
        fs::create_dir_all(&ab)?;
        fs::create_dir_all(&abc)?;

        let a_dot_dir = a.join(TEST.dot_dir());
        let ab_dot_dir = ab.join(TEST.dot_dir());
        let abc_dot_dir = abc.join(TEST.dot_dir());
        fs::create_dir_all(&a_dot_dir)?;
        fs::create_dir_all(&ab_dot_dir)?;
        fs::create_dir_all(&abc_dot_dir)?;

        // .test
        // a/.sl
        // a/b/.test
        // a/b/c/.sl
        let sniff_roots_result = sniff_roots(&abc)?;
        assert_eq!(sniff_roots_result.len(), 4);
        assert_eq!(sniff_roots_result[0].0, abc);
        assert_eq!(sniff_roots_result[0].1.repo, TEST.repo);
        assert_eq!(sniff_roots_result[1].0, ab);
        assert_eq!(sniff_roots_result[1].1.repo, TEST.repo);
        assert_eq!(sniff_roots_result[2].0, a);
        assert_eq!(sniff_roots_result[2].1.repo, TEST.repo);
        assert_eq!(sniff_roots_result[3].0, dir.path());
        assert_eq!(sniff_roots_result[3].1.repo, TEST.repo);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_sniff_root_symlink_into_repo() -> Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir()?;

        let root = dir.path().join("root");

        let dot_dir = root.join(TEST.dot_dir());
        fs::create_dir_all(dot_dir)?;

        let subdir = root.join("subdir");
        fs::create_dir_all(&subdir)?;

        let link = dir.path().join("link");
        symlink(&subdir, &link)?;

        let (sniffed_root, _) = sniff_root(&link)?.unwrap();
        assert_eq!(sniffed_root, root);
        let sniff_roots_result = sniff_roots(&link)?;
        assert_eq!(sniff_roots_result.len(), 1);
        assert_eq!(sniff_roots_result[0].0, root);

        let relative_link = dir.path().join("relative-link");
        symlink(Path::new("root/subdir"), &relative_link)?;

        let (sniffed_root, _) = sniff_root(&relative_link)?.unwrap();
        assert_eq!(sniffed_root, root);
        let sniff_roots_result = sniff_roots(&relative_link)?;
        assert_eq!(sniff_roots_result.len(), 1);
        assert_eq!(sniff_roots_result[0].0, root);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_sniff_root_symlink_within_repo() -> Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir()?;

        let root = dir.path().join("root");

        let dot_dir = root.join(TEST.dot_dir());
        fs::create_dir_all(dot_dir)?;

        let outside_repo = dir.path().join("not_repo");
        fs::create_dir_all(&outside_repo)?;

        let link_within_repo = root.join("link");
        symlink(&outside_repo, &link_within_repo)?;

        let (sniffed_root, _) = sniff_root(&link_within_repo)?.unwrap();
        assert_eq!(sniffed_root, root);
        let sniff_roots_result = sniff_roots(&link_within_repo)?;
        assert_eq!(sniff_roots_result.len(), 1);
        assert_eq!(sniff_roots_result[0].0, root);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_sniff_root_symlink_within_repo_into_another_repo() -> Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir()?;

        let repo1 = dir.path().join("repo1");
        fs::create_dir_all(repo1.join(TEST.dot_dir()))?;

        let repo2 = dir.path().join("repo2");
        fs::create_dir_all(repo2.join(TEST.dot_dir()))?;

        let repo2_subdir = repo2.join("subdir");
        fs::create_dir_all(&repo2_subdir)?;

        let link_within_repo = repo1.join("link");
        symlink(&repo2_subdir, &link_within_repo)?;

        // We have a symlink within repo1 pointing to a subdir in repo2. We should prefer
        // the "lexical" containing repo indicated by the path.
        let (sniffed_root, _) = sniff_root(&link_within_repo)?.unwrap();
        assert_eq!(sniffed_root, repo1);
        let sniff_roots_result = sniff_roots(&link_within_repo)?;
        assert_eq!(sniff_roots_result.len(), 1);
        assert_eq!(sniff_roots_result[0].0, repo1);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_sniff_root_symlink_cycle() -> Result<()> {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir()?;

        let a = dir.path().join("a");
        let b = dir.path().join("b");

        symlink(b.join("subdir"), &a)?;
        symlink(a.join("subdir"), &b)?;

        assert!(sniff_root(&a)?.is_none());
        assert_eq!(sniff_roots(&a)?.len(), 0);

        Ok(())
    }

    #[test]
    fn test_sniff_root_priority() -> Result<()> {
        let dir = tempfile::tempdir()?;

        // .test      (pri: 5)
        // a/.sl      (pri: 0, highest)
        // a/b/.test  (pri: 5)
        // a/b/c

        let dir = dir.path();
        let dir_a = dir.join("a");
        let dir_b = dir_a.join("b");
        let dir_c = dir_b.join("c");

        fs::create_dir_all(dir.join(TEST.repo.sniff_dot_dir()))?;
        fs::create_dir_all(dir_a.join(SL.repo.sniff_dot_dir()))?;
        write_required_files(&dir_a, SL);
        fs::create_dir_all(dir_b.join(TEST.repo.sniff_dot_dir()))?;
        fs::create_dir_all(&dir_c)?;

        assert_eq!(sniff_root(dir)?.unwrap().1.repo, TEST.repo);
        assert_eq!(sniff_root(&dir_c)?.unwrap().1.repo, SL.repo);
        assert_eq!(sniff_root(&dir_b)?.unwrap().1.repo, SL.repo);
        assert_eq!(sniff_root(&dir_a)?.unwrap().1.repo, SL.repo);

        assert_eq!(sniff_dir(dir)?.unwrap().repo, TEST.repo);
        assert_eq!(sniff_dir(&dir_a)?.unwrap().repo, SL.repo);
        assert_eq!(sniff_dir(&dir_b)?.unwrap().repo, TEST.repo);
        assert!(sniff_dir(&dir_c)?.is_none());

        Ok(())
    }

    #[test]
    fn test_sniff_required_files() -> Result<()> {
        // a/.sl: valid (contains "requires")
        // a/b/.sl: invalid (no "requires")
        let dir = tempfile::tempdir()?;
        let dir = dir.path();

        let dir_a = dir.join("a");
        let dir_a_b = dir_a.join("b");
        fs::create_dir_all(dir_a_b.join(SL.dot_dir()))?;
        fs::create_dir_all(dir_a.join(SL.dot_dir()))?;
        fs::write(dir_a.join(SL.dot_dir()).join("requires"), b"store")?;

        // sniff_root should ignore a/b/.sl (no "requires") and use a/.sl (has "requires").
        let sniffed_path = sniff_root(&dir_a_b)?.unwrap().0;
        assert_eq!(sniffed_path, dir_a);

        Ok(())
    }

    #[test]
    fn test_dotgit_submodule() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let dir = dir.path();

        let git_module_dir = dir.join(".git").join("modules").join("sub1");
        fs::create_dir_all(&git_module_dir)?;

        let submodule_dir = dir.join("sub1");
        fs::create_dir_all(&submodule_dir)?;
        fs::write(submodule_dir.join(".git"), "gitdir: ../.git/modules/sub1")?;

        let id = sniff_dir(dir)?.unwrap();
        assert_eq!(id.repo, SL_GIT.repo);

        let full_dot_dir = id.resolve_full_dot_dir(&submodule_dir);
        assert_eq!(full_dot_dir, git_module_dir.join("sl"));

        Ok(())
    }

    #[test]
    fn test_split_rcpath() {
        let rcpath = [
            "sys=111", "user=222", "sys=333", "user=444", "555", "foo=666",
        ]
        .join(";");
        let t = |prefix_list| -> Vec<&str> { split_rcpath(&rcpath, prefix_list).collect() };
        assert_eq!(t(&["sys", ""]), ["111", "333", "555", "foo=666"]);
        assert_eq!(t(&["user"]), ["222", "444"]);
    }

    fn write_required_files(dir: &Path, ident: Identity) {
        for path in ident.repo.sniff_dot_dir_required_files {
            fs::write(dir.join(ident.repo.sniff_dot_dir()).join(path), b"x").unwrap();
        }
    }
}
