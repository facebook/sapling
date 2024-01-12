/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::env::VarError;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct Identity {
    user: UserIdentity,
    repo: RepoIdentity,
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

#[derive(PartialEq, Debug, Clone, Copy)]
struct RepoIdentity {
    /// Metadata directory of the current identity. If this directory exists in the current repo, it
    /// implies that the repo is using this identity.
    dot_dir: &'static str,

    /// Config file for the repo; located inside of `dot_dir`.
    ///
    /// Examples: `config`, `hgrc`
    config_repo_file: &'static str,
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

    pub fn dot_dir(&self) -> &'static str {
        self.repo.dot_dir
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
            Err(err) if err == VarError::NotPresent => None,
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
    user: UserIdentity {
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

    repo: RepoIdentity {
        dot_dir: ".hg",
        config_repo_file: "hgrc",
    },
};

const SL: Identity = Identity {
    user: UserIdentity {
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

    repo: RepoIdentity {
        dot_dir: ".sl",
        config_repo_file: "config",
    },
};

#[cfg(test)]
const TEST: Identity = Identity {
    user: UserIdentity {
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

    repo: RepoIdentity {
        dot_dir: ".test",
        config_repo_file: "config",
    },
};

#[cfg(all(not(feature = "sl_only"), not(test)))]
mod idents {
    use super::*;

    pub fn all() -> &'static [Identity] {
        &[SL, HG]
    }
}

#[cfg(feature = "sl_only")]
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

static DEFAULT: Lazy<RwLock<Identity>> = Lazy::new(|| RwLock::new(compute_default()));

pub use idents::all;

pub fn default() -> Identity {
    *DEFAULT.read()
}

pub fn reset_default() {
    *DEFAULT.write() = compute_default();
}

/// Default `Identity` based on the current executable name.
fn compute_default() -> Identity {
    let path = std::env::current_exe().expect("current_exe() should not fail");
    let file_name = path
        .file_name()
        .expect("file_name() on current_exe() should not fail");
    let file_name = file_name.to_string_lossy();
    let (ident, reason) = (|| {
        let env_override = all()
            .iter()
            .find_map(|id| id.env_var("IDENTITY"))
            .and_then(|v| v.ok());

        for ident in all() {
            if Some(ident.user.cli_name) == env_override.as_deref() {
                return (*ident, "env var");
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

    tracing::info!(
        identity = ident.user.cli_name,
        argv0 = file_name.as_ref(),
        reason,
        "identity from argv0"
    );

    ident
}

/// CLI name to be used in user facing messaging.
pub fn cli_name() -> &'static str {
    DEFAULT.read().cli_name()
}

/// Sniff the given path for the existence of "{path}/.hg" or
/// "{path}/.sl" directories, yielding the sniffed Identity, if any.
/// Only permissions errors are propagated.
pub fn sniff_dir(path: &Path) -> Result<Option<Identity>> {
    for id in all() {
        let test_path = path.join(id.repo.dot_dir);
        tracing::trace!(path=%path.display(), "sniffing dir");
        match fs::metadata(&test_path) {
            Ok(md) if md.is_dir() => {
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
///  containing a ".hg" or ".sl" directory. The ancestor directory and
///  corresponding Identity are returned, if any. Only permission
///  errors are propagated.
pub fn sniff_root(path: &Path) -> Result<Option<(PathBuf, Identity)>> {
    tracing::debug!(start=%path.display(), "sniffing for repo root");

    let mut path = Some(path);

    while let Some(p) = path {
        if let Some(ident) = sniff_dir(p)? {
            return Ok(Some((p.to_path_buf(), ident)));
        }

        path = p.parent();
    }

    Ok(None)
}

pub fn env_var(var_suffix: &str) -> Option<Result<String, VarError>> {
    let current_id = DEFAULT.read();

    // Always prefer current identity.
    if let Some(res) = current_id.env_var(var_suffix) {
        return Some(res);
    }

    // Backwards compat for old env vars.
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

        assert!(sniff_root(&root)?.is_none());

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
    fn test_split_rcpath() {
        let rcpath = [
            "sys=111", "user=222", "sys=333", "user=444", "555", "foo=666",
        ]
        .join(";");
        let t = |prefix_list| -> Vec<&str> { split_rcpath(&rcpath, prefix_list).collect() };
        assert_eq!(t(&["sys", ""]), ["111", "333", "555", "foo=666"]);
        assert_eq!(t(&["user"]), ["222", "444"]);
    }
}
