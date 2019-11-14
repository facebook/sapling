/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This is "scratch", a tool for locating and creating scratch space.
//! Why not just use the "mktemp" utility?  Scratch creates a persistent
//! and deterministic scratch location for a given input path.  This is
//! useful for holding build artifacts without having them clog up the
//! repository.  In addition, "scratch" is aware that sometimes we
//! may want to use watchman to watch a portion of the scratch space
//! and can arrange the directory structure to prevent over-watching.

use clap::{App, AppSettings, Arg, SubCommand};
use failure::{bail, format_err, Error, Fallible};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::prelude::*;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use telemetry::hostinfo;
use telemetry::repoinfo;

/// The configuration is intentionally very minimal, and explicitly
/// not made accessible via command line options; the intent is that
/// placement of the scratch space is the policy of the owner of the
/// environment rather than a decision made by the tool that needs
/// the scratch space.
/// Configuration is loaded by parsing the following files as TOML
/// and overlaying the values from the later files over the the
/// current parsed state:
/// * The file /etc/scratch.toml
/// * The file ~/.scratch.toml
/// * The file identified by the $SCRATCH_CONFIG_PATH environmental
///   variable.
/// Example configuration file might look like:
///
/// ```
/// template = "/data/users/$REPO_OWNER_USER/scratch"
/// overrides = {"/data/users/wez/fbsource": "/dev/shm/scratch"}
/// ```
#[derive(Debug, Deserialize, Default)]
struct Config {
    /// An optional "template" path.  Template paths are subject to
    /// two simple substitution transformations; $HOME is expanded
    /// to the home directory of the current user and $USER is
    /// expanded to the user name of the current user.  This allows
    /// definition of a simple placement policy without explicitly
    /// specifying the value for every user.
    /// If left unspecified, the default value is equivalent to
    /// `$HOME/.scratch`.
    template: Option<String>,

    /// The list of overridden settings
    overrides: HashMap<String, String>,
}

/// Returns the home directory of the user as a string.
/// Will panic if it cannot be resolved, or cannot be represented
/// as UTF-8.
fn home_dir() -> String {
    let home = dirs::home_dir().expect("resolved HOME dir");
    home.to_str()
        .expect(&format!(
            "HOME dir {:?} was not representable as UTF-8",
            home
        ))
        .into()
}

#[cfg(unix)]
fn lookup_home_dir_for_user(user: &str) -> Fallible<String> {
    let pw = PasswordEntry::by_name(user)?;
    Ok(pw.home_dir)
}

/// This is technically wrong for windows, but is at least
/// wrong in a backwards compatible way
#[cfg(windows)]
fn lookup_home_dir_for_user(user: &str) -> Fallible<String> {
    Ok(home_dir())
}

impl Config {
    /// Attempt to load a Config instance from the specified path.
    /// If path does not exist, None is returned.
    fn load_file<P: AsRef<Path>>(path: P) -> Result<Option<Self>, Error> {
        let path = path.as_ref();
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => return Ok(None),
                _ => bail!(err),
            },
        };
        let mut s = String::new();
        file.read_to_string(&mut s)?;

        toml::from_str(&s)
            .map(|c| Some(c))
            .map_err(|e| format_err!("error while loading TOML from {}: {:?}", path.display(), e))
    }

    /// Merge the values from other into self.
    fn merge(&mut self, mut other: Self) {
        if let Some(template) = other.template.take() {
            self.template = Some(template);
        }

        self.overrides.extend(other.overrides.into_iter());
    }

    /// Compute the effective configuration by loading the configuration
    /// files in order and merging them together.  Missing files are OK,
    /// but any IO or parse errors cause the config resolution to stop and
    /// return the error.
    fn load() -> Result<Self, Error> {
        let mut result = Self::default();

        let config_files = [
            #[cfg(unix)]
            Some("/etc/scratch.toml".into()),
            #[cfg(windows)]
            Some("C:/ProgramData/facebook/scratch.toml".into()),
            Some(format!("{}/.scratch.toml", home_dir())),
            std::env::var("SCRATCH_CONFIG_PATH").ok(),
        ];
        for path in config_files.iter().filter_map(Option::as_ref) {
            if let Some(o) = Self::load_file(path)? {
                result.merge(o);
            }
        }

        Ok(result)
    }

    /// Look up the template string for a given repo path.
    /// This is taken from a matching `overrides` entry first, if any,
    /// then the global `template` configuration, if any, finally
    /// falling back to a default value of `$HOME/.scratch`.
    /// We use `$HOME` rather than `/tmp` as it less prone to
    /// bad actors mounting a symlink attack.
    fn template_for_path(&self, path: &Path, owner: &str) -> String {
        // First, let's see if we have an override for this path
        let path_str = path.to_str().expect("path must be UTF-8");
        if let Some(over) = self.overrides.get(path_str) {
            return over.clone();
        }
        match &self.template {
            &Some(ref s) => s.clone(),
            &None => {
                // This is a little bit of a hack; ideally we'd
                // configure this in chef, but don't have bandwidth
                // to prepare a recipe for this in time; will follow
                // up in T31633485.
                // If there is a /data/users/<owner> dir, then we
                // use that to hold the scratch dir.
                let local = format!("/data/users/{}", owner);
                if let Ok(meta) = fs::metadata(&local) {
                    if meta.is_dir() {
                        return format!("{}/scratch", local);
                    }
                }
                // Otherwise use their home dir
                format!("{}/.scratch", home_dir())
            }
        }
    }
}

fn run() -> Result<(), Error> {
    let matches = App::new("Scratch")
        .setting(AppSettings::SubcommandRequired)
        .setting(AppSettings::ColoredHelp)
        .version("1.0")
        .author("Source Control <oncall+source_control@xmail.facebook.com")
        .arg(
            Arg::with_name("no-create")
                .long("no-create")
                .short("n")
                .help("Do not create files or directories"),
        )
        .subcommand(
            SubCommand::with_name("path")
                .about("create and display the scratch path corresponding to the input path")
                .arg(
                    Arg::with_name("subdir")
                        .long("subdir")
                        .help("generate an isolated subdir based off this string")
                        .value_name("PATH"),
                )
                .arg(
                    Arg::with_name("watchable")
                        .long("watchable")
                        .help("the returned scratch space needs to be watchable by watchman"),
                )
                .arg(
                    Arg::with_name("REPO")
                        .help(
                            "Specifies the path to the repo. \
                             If omitted, infer the path from the current working directory",
                        )
                        .index(1),
                ),
        )
        .get_matches();

    let no_create = matches.is_present("no-create");

    let config = Config::load()?;

    match matches.subcommand() {
        ("path", Some(cmd)) => {
            let subdir = cmd.value_of("subdir");
            let watchable = cmd.is_present("watchable");
            let repo = cmd.value_of("REPO");
            path_command(&config, no_create, subdir, watchable, repo)
        }
        // AppSettings::SubcommandRequired should mean that this is unpossible
        _ => unreachable!("wut?"),
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(err) => {
            eprintln!("scratch failed: {}", err);
            std::process::exit(1)
        }
    }
}

/// Returns the current username, falling back to the literal
/// string `$USER` for env var expansion.
fn get_current_user() -> String {
    hostinfo::get_user_name().unwrap_or("$USER".into())
}

#[cfg(unix)]
struct PasswordEntry {
    unixname: String,
    home_dir: String,
    uid: u32,
    gid: u32,
}

#[cfg(unix)]
impl PasswordEntry {
    fn maybe_string(cstr: *const libc::c_char, context: &str) -> Fallible<String> {
        if cstr.is_null() {
            Err(failure::err_msg(context.to_string()))
        } else {
            let cstr = unsafe { std::ffi::CStr::from_ptr(cstr) };
            cstr.to_str().map_err(|e| e.into()).map(|x| x.to_owned())
        }
    }

    fn from_password(pwent: *const libc::passwd) -> Fallible<Self> {
        failure::ensure!(!pwent.is_null(), "password ptr is null");
        let pw = unsafe { &*pwent };
        Ok(Self {
            unixname: Self::maybe_string(pw.pw_name, "pw_name is null")?,
            home_dir: Self::maybe_string(pw.pw_dir, "pw_dir is null")?,
            uid: pw.pw_uid,
            gid: pw.pw_gid,
        })
    }

    /// Lookup a PasswordEntry for a uid.
    /// Not thread safe.
    pub fn by_uid(uid: u32) -> Fallible<Self> {
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            let err = std::io::Error::last_os_error();
            bail!("getpwuid({}) failed: {}", uid, err);
        }
        Self::from_password(pw)
    }

    /// Lookup a PasswordEntry for a unix username.
    /// Not thread safe.
    pub fn by_name(unixname: &str) -> Fallible<Self> {
        let user_cstr = std::ffi::CString::new(unixname.to_string())?;

        let pw = unsafe { libc::getpwnam(user_cstr.as_ptr()) };
        if pw.is_null() {
            let err = std::io::Error::last_os_error();
            bail!("getpwnam({}) failed: {}", unixname, err);
        }

        Self::from_password(pw)
    }
}

/// Given a path, return the unix name of the owner of that path.
/// If we cannot stat the path, raise an error.
#[cfg(unix)]
fn get_file_owner(path: &Path) -> Result<String, Error> {
    let meta = fs::metadata(path)
        .map_err(|e| format_err!("unable to get metadata for {}: {}", path.display(), e))?;
    let uid = meta.uid();
    let pw = PasswordEntry::by_uid(uid)?;

    Ok(pw.unixname)
}

#[cfg(unix)]
fn set_file_owner(path: &Path, owner: &str) -> Fallible<()> {
    use std::ffi::CString;

    let is_root = unsafe { libc::geteuid() } == 0;

    if !is_root {
        // Can't change the ownership, so stick with who we are
        return Ok(());
    }

    let pw = PasswordEntry::by_name(owner)?;

    let path_cstr =
        CString::new(path.to_str().ok_or_else(|| {
            format_err!("path {} cannot be represented as String", path.display())
        })?)?;
    let result = unsafe { libc::chown(path_cstr.as_ptr(), pw.uid, pw.gid) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        bail!(
            "Failed to chown({}, {} (uid={}, gid={})): {}",
            path.display(),
            owner,
            pw.uid,
            pw.gid,
            err
        );
    }

    Ok(())
}

/// This should alter the file ACLs on windows, but for now we're just
/// ignoring this, as we don't think the issue a practical problem.
#[cfg(windows)]
fn set_file_owner(_path: &Path, _owner: &str) -> Fallible<()> {
    Ok(())
}

/// This should return the owner of a path, but for now it just returns
/// the current user name on Windows systems.  This is probably correct
/// and good enough for the moment, and we can add support for the real
/// thing in a later diff.
#[cfg(windows)]
fn get_file_owner(_path: &Path) -> Result<String, Error> {
    Ok(get_current_user())
}

/// Resolves the root directory to use as the scratch space for a given
/// repository path.  This is the function that performs expansion of
/// the $USER and $HOME placeholder tokens in the configured template.
fn scratch_root(config: &Config, path: &Path) -> Result<PathBuf, Error> {
    let repo_owner = get_file_owner(path)?;
    let template = config.template_for_path(path, &repo_owner);

    let user = get_current_user();
    let home = home_dir();
    let repo_owner_home = lookup_home_dir_for_user(&repo_owner)?;

    let mut root = PathBuf::from(
        template
            .replace("$REPO_OWNER_USER", &repo_owner)
            .replace("$REPO_OWNER_HOME", &repo_owner_home)
            .replace("$USER", &user)
            .replace("$HOME", &home),
    );

    root.push(encode(
        path.to_str()
            .ok_or(format_err!("{:?} cannot be converted to utf8", path))?,
    ));
    Ok(root)
}

/// A watchable path needs a .watchmanconfig file to define the boundary
/// of the watch and allow the watch of occur.
fn create_watchmanconfig(_config: &Config, path: &Path, repo_owner: &str) -> Result<(), Error> {
    let filename = path.join(".watchmanconfig");
    let mut file = fs::File::create(&filename)?;
    // Write out an empty json object
    file.write_all("{}".as_bytes())?;
    set_file_owner(&filename, repo_owner)?;
    Ok(())
}

/// Performs the `path` command
fn path_command(
    config: &Config,
    no_create: bool,
    subdir: Option<&str>,
    watchable: bool,
    path: Option<&str>,
) -> Result<(), Error> {
    // Canonicalize the provided path.  If no path was provided, fall
    // back to the cwd.
    let path = match path {
        Some(path) => fs::canonicalize(path)
            .map_err(|e| format_err!("unable to canonicalize path: {}: {}", path, e))?,
        None => env::current_dir()?,
    };

    // Resolve the path to the corresponding repo root.
    // If the path is not a repo then we use the provided path.
    let repo_root = match repoinfo::locate_repo_root_and_type(&path) {
        Some((path, _)) => path,
        None => &path,
    };

    // Get the base scratch path for this repo
    let mut result = scratch_root(&config, repo_root)?;
    let repo_owner = get_file_owner(repo_root)?;

    // If they asked for a subdir, compute it
    if let Some(subdir) = subdir {
        if watchable {
            result.push("watchable");
        }
        result.push(encode(subdir));
    }

    if !no_create {
        fs::create_dir_all(&result)?;
        set_file_owner(&result, &repo_owner)?;
        if watchable {
            create_watchmanconfig(&config, &result, &repo_owner)?;
        }
    }

    println!("{}", result.display());
    Ok(())
}

/// Given a string representation of a path, encode it such that all
/// file/path special characters are replaced with non-special characters.
/// This has the effect of flattening a relative path fragment like
/// `foo/bar` into a single level path component like `fooZbar`.
/// Scratch uses this to give the appearance of hierarchy to clients
/// without having an actual hierarchy.  This is important on systems
/// such as Windows and macOS where the filesystem watchers are always
/// recursive.
/// The mapping is not and does not need to be reversible.
/// Why not just compute a SHA or MD5 hash?  It is nicer for the user
/// to have an idea of what the path is when they list the scratch container
/// path, which is something they'll likely end up doing when their disk
/// gets full, and they'll appreciate knowing which of these dirs have
/// value to them.
fn encode(path: &str) -> String {
    let mut result = String::with_capacity(path.len());

    for (i, b) in path.chars().enumerate() {
        if cfg!(unix) && i == 0 && b == '/' {
            // On unix, most paths begin with a slash, which
            // means that we'd use a Z prefix everything.
            // Let's just skip the first character.
            continue;
        }
        match b {
            '/' | '\\' => result.push('Z'),
            'Z' => result.push_str("_Z"),
            ':' => result.push_str("_"),
            _ => result.push(b),
        }
    }

    result
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_encode() {
        assert_eq!(encode("/foo/bar"), "fooZbar");
        assert_eq!(encode("foo"), "foo");
        assert_eq!(encode("foo/bar"), "fooZbar");
        assert_eq!(encode(r"foo\bar"), "fooZbar");
        assert_eq!(encode("fooZbar"), "foo_Zbar");
        assert_eq!(encode("foo_Zbar"), "foo__Zbar");
        assert_eq!(encode(r"C:\foo\bar"), "C_ZfooZbar");
        assert_eq!(encode(r"\\unc\path"), "ZZuncZpath");
    }
}
