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
    /// Name of the binary. Used for showing help messages
    ///
    /// Example: `Checkout failed. Resume with 'sl checkout --continue'`
    cli_name: &'static str,

    /// Name of the product
    product_name: &'static str,

    /// Full name of the product
    long_product_name: &'static str,

    /// Metadata directory of the current identity. If this directory exists in the current repo, it
    /// implies that the repo is using this identity.
    dot_dir: &'static str,

    /// Prefix of environment variables related to current repo. To be used in the future.
    env_prefix: &'static str,

    /// Main config file for some identities. Located inside of home directory (e.g. `/home/alice/.hgrc`)
    config_name: &'static str,

    /// Subdirectory of user's cache directory used for config. The parent of this directory can change
    /// depending on the operating system
    ///
    /// | OS       | Value of parent directory           | Example of parent + config_directory     |
    /// |----------|-------------------------------------|------------------------------------------|
    /// | Linux    | `$XDG_CACHE_HOME` or `$HOME`/.cache | /home/alice/.config/sapling              |
    /// | macOS    | `$HOME`/Library/Caches              | /Users/Alice/Library/Preferences/sapling |
    /// | Windows  | `{FOLDERID_LocalAppData}`           | C:\Users\Alice\AppData\Local\sapling     |
    config_directory: &'static str,

    /// Main or secondary config file (depends on the identity); located inside of `config_directory`
    config_main_file: &'static str,

    /// Config file for the repo; located inside of `dot_dir`.
    ///
    /// Examples: `.sl/config`, `.hg/hgrc`
    config_repo_file: &'static str,

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

impl Identity {
    pub fn cli_name(&self) -> &'static str {
        self.cli_name
    }

    pub fn product_name(&self) -> &'static str {
        self.product_name
    }

    pub fn long_product_name(&self) -> &'static str {
        self.long_product_name
    }

    pub fn dot_dir(&self) -> &'static str {
        self.dot_dir
    }

    pub fn config_directory(&self) -> &'static str {
        self.config_directory
    }

    pub fn config_name(&self) -> &'static str {
        self.config_name
    }

    pub fn config_main_file(&self) -> &'static str {
        self.config_main_file
    }

    pub fn config_repo_file(&self) -> &'static str {
        self.config_repo_file
    }

    pub fn env_prefix(&self) -> &'static str {
        self.env_prefix
    }

    pub const fn env_name_static(&self, suffix: &str) -> Option<&'static str> {
        // Use byte slice to workaround const_fn limitation.
        let bsuffix = suffix.as_bytes();
        match bsuffix {
            b"CONFIG" => Some(self.scripting_config_env_var),
            b"PLAIN" => Some(self.scripting_env_var),
            b"PLAINEXCEPT" => Some(self.scripting_except_env_var),
            _ => None,
        }
    }

    pub fn env_name(&self, suffix: &str) -> Cow<'static, str> {
        match self.env_name_static(suffix) {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned([self.env_prefix, suffix].concat()),
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
        let mut paths = vec![];
        if self.product_name() == "Mercurial" {
            // ~/.hgrc and ~/mercurial.ini are legacy paths that we support only when the current identity is HG
            if let Some(home_dir) = dirs::home_dir() {
                paths.push(home_dir.join(format!(".{}", self.config_name())));
                #[cfg(windows)]
                {
                    paths.push(home_dir.join("mercurial.ini"));
                }
            }
        }
        if let Some(config_dir) = dirs::config_dir() {
            paths.push(
                config_dir
                    .join(self.config_directory())
                    .join(self.config_main_file()),
            )
        }
        paths
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cli_name)
    }
}

const HG: Identity = Identity {
    cli_name: "hg",
    product_name: "Mercurial",
    long_product_name: "Mercurial Distributed SCM",
    dot_dir: ".hg",
    env_prefix: "HG",
    config_name: "hgrc",
    config_directory: "hg",
    config_main_file: "hgrc",
    config_repo_file: "hgrc",
    scripting_env_var: "HGPLAIN",
    scripting_config_env_var: "HGRCPATH",
    scripting_except_env_var: "HGPLAINEXCEPT",
};

const SL: Identity = Identity {
    cli_name: "sl",
    product_name: "Sapling",
    long_product_name: "Sapling SCM",
    dot_dir: ".sl",
    env_prefix: "SL",
    config_name: "slconfig",
    config_directory: "sapling",
    config_main_file: "sapling.conf",
    config_repo_file: "config",
    scripting_env_var: "SL_AUTOMATION",
    scripting_config_env_var: "SL_CONFIG_PATH",
    scripting_except_env_var: "SL_AUTOMATION_EXCEPT",
};

#[cfg(test)]
const TEST: Identity = Identity {
    cli_name: "test",
    product_name: "Test",
    long_product_name: "Testing SCM",
    dot_dir: ".test",
    env_prefix: "TEST",
    config_name: "testrc",
    config_directory: "test",
    config_main_file: "test.conf",
    config_repo_file: "config",
    scripting_env_var: "TEST_SCRIPT",
    scripting_config_env_var: "TEST_RC_PATH",
    scripting_except_env_var: "TEST_SCRIPT_EXCEPT",
};

#[cfg(all(not(feature = "sl_only"), not(test)))]
pub mod idents {
    use super::*;
    pub static ALL_IDENTITIES: &[Identity] = &[HG, SL];

    /// Default `Identity` based on the current executable name.
    pub static DEFAULT: Lazy<Identity> = Lazy::new(|| {
        let path = std::env::current_exe().expect("current_exe() should not fail");
        let file_name = path
            .file_name()
            .expect("file_name() on current_exe() should not fail");
        let file_name = file_name.to_string_lossy();
        let (ident, reason) = (|| {
            for ident in ALL_IDENTITIES {
                if file_name.contains(ident.cli_name) {
                    return (*ident, "contains");
                }
            }
            // Special case: for fbcode/eden/testlib/ tests the "current_exe"
            // could be "python3.8". Use "hg" to maintain test compatibility.
            // If we updated the tests, the special case can be dropped.
            if file_name.starts_with("python") {
                return (HG, "python");
            }
            // Fallback to SL if current_exe does not provide information.
            (SL, "fallback")
        })();
        tracing::info!(
            id = SL.cli_name,
            argv0 = file_name.as_ref(),
            reason = reason,
            "identity from argv0"
        );
        ident
    });
}

#[cfg(feature = "sl_only")]
pub mod idents {
    use super::*;
    pub static DEFAULT: Lazy<Identity> = Lazy::new(|| SL);
    pub static ALL_IDENTITIES: &[Identity] = &[SL];
}

#[cfg(test)]
pub mod idents {
    use super::*;
    pub static DEFAULT: Lazy<Identity> = Lazy::new(|| HG);
    pub static ALL_IDENTITIES: &[Identity] = &[HG, SL, TEST];
}

pub static IDENTITY: Lazy<RwLock<Identity>> = Lazy::new(|| RwLock::new(*idents::DEFAULT));

/// CLI name to be used in user facing messaging.
pub fn cli_name() -> &'static str {
    IDENTITY.read().cli_name
}

/// Sniff the given path for the existence of "{path}/.hg" or
/// "{path}/.sl" directories, yielding the sniffed Identity, if any.
/// Only permissions errors are propagated.
pub fn sniff_dir(path: &Path) -> Result<Option<Identity>> {
    for id in idents::ALL_IDENTITIES {
        let test_path = path.join(id.dot_dir);
        tracing::trace!(path=%path.display(), "sniffing dir");
        match fs::metadata(&test_path) {
            Ok(md) if md.is_dir() => {
                tracing::debug!(id=%id, path=%path.display(), "sniffed repo dir");
                return Ok(Some(*id));
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
    let current_id = IDENTITY.read();

    // Always prefer current identity.
    if let Some(res) = current_id.env_var(var_suffix) {
        return Some(res);
    }

    // Backwards compat for old env vars.
    for id in idents::ALL_IDENTITIES {
        if *current_id == *id {
            continue;
        }

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

pub fn sniff_env() -> Identity {
    if let Ok(id_name) = try_env_var("IDENTITY") {
        for id in idents::ALL_IDENTITIES {
            if id.cli_name == id_name {
                tracing::info!(identity = id.cli_name, "sniffed identity from env");
                return *id;
            }
        }
    }

    *idents::DEFAULT
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
            fs::create_dir_all(root.join(idents::DEFAULT.dot_dir()))?;

            assert_eq!(sniff_dir(&root)?.unwrap(), *idents::DEFAULT);
        }

        {
            let root = dir.path().join("test1");
            fs::create_dir_all(root.join(TEST.dot_dir()))?;

            assert_eq!(sniff_dir(&root)?.unwrap(), TEST);
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
            let dot_dir = root.join(idents::DEFAULT.dot_dir());
            fs::create_dir_all(&dot_dir)?;

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
        fs::create_dir_all(&dot_dir)?;

        assert_eq!(sniff_root(&root)?.unwrap(), (root.clone(), TEST));

        let abc = root.join("a/b/c");
        fs::create_dir_all(&abc)?;

        assert_eq!(sniff_root(&abc)?.unwrap(), (root, TEST));

        Ok(())
    }
}
