/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;
use std::sync::RwLock;

use configmodel::Config;
use configmodel::ConfigExt;
use identity::dotgit::follow_dotgit_path;
use spawn_ext::CommandExt;
use types::HgId;

/// Run `git` outside a repo.
#[derive(Default, Clone)]
pub struct GlobalGit {
    config: RunGitConfig,

    /// Extra Git configs, "foo.bar=baz".
    pub extra_git_configs: Vec<String>,
}

/// Run `git` in a "bare" git repo, without a working copy.
pub struct BareGit {
    /// The `GIT_DIR`.
    /// This is usually `root/.git`. When `.git` is a "symlink" ("gitdir: ..."),
    /// this is the "symlink" destination.
    pub(crate) git_dir: PathBuf,
    pub(crate) parent: GlobalGit,
}

/// Run `git` in a "regular" repo with a working copy.
pub struct RepoGit {
    /// cwd used when running commands.
    /// This is the working copy root.
    root: PathBuf,
    pub(crate) parent: BareGit,
    /// TODO: remove after fast path rollout
    pub index_fast_path: bool,
}

/// Config related to run git.
#[derive(Clone)]
struct RunGitConfig {
    /// Path to the "git" command.
    pub git_binary: String,

    /// Whether to use --verbose.
    pub verbose: bool,

    /// Whether to use --quiet.
    pub quiet: bool,
}

impl Default for RunGitConfig {
    fn default() -> Self {
        if let Ok(config) = DEFAULT_CONFIG.read() {
            if let Some(config) = config.as_ref() {
                return config.clone();
            }
        }
        Self {
            git_binary: GIT.to_owned(),
            verbose: false,
            quiet: false,
        }
    }
}

impl RunGitConfig {
    fn from_config(config: &dyn Config) -> Self {
        let (git_binary, verbose, quiet) = (
            config
                .get_or("ui", "git", || GIT.to_owned())
                .unwrap_or_else(|_| GIT.to_owned()),
            config.get_or_default("ui", "verbose").unwrap_or_default(),
            config.get_or_default("ui", "quiet").unwrap_or_default(),
        );
        Self {
            git_binary,
            verbose,
            quiet,
        }
    }
}

static DEFAULT_CONFIG: RwLock<Option<RunGitConfig>> = RwLock::new(None);

impl GlobalGit {
    /// Construct from config.
    pub fn from_config(config: &dyn Config) -> Self {
        let config = RunGitConfig::from_config(config);
        Self {
            config,
            ..Default::default()
        }
    }

    /// Associate with a bare repo.
    pub fn with_bare(self, git_dir: PathBuf) -> BareGit {
        BareGit {
            git_dir: follow_dotgit_path(git_dir),
            parent: self,
        }
    }

    /// Associate with a regular repo.
    pub fn with_repo(self, root: PathBuf) -> RepoGit {
        let git_dir = root.join(".git");
        self.with_bare(git_dir).with_working_copy(root)
    }

    /// Set default config. Affects constructors without `config` like `GlobalGit::default`.
    pub fn set_default_config(config: &dyn Config) {
        *DEFAULT_CONFIG.write().unwrap() = Some(RunGitConfig::from_config(config));
    }
}

impl BareGit {
    /// Construct from git_dir (".git" path) and config.
    pub fn from_git_dir_and_config(git_dir: PathBuf, config: &dyn Config) -> Self {
        Self {
            git_dir: follow_dotgit_path(git_dir),
            parent: GlobalGit::from_config(config),
        }
    }

    /// Construct from git_dir (".git" path) and default config.
    pub fn from_git_dir(git_dir: PathBuf) -> Self {
        Self {
            git_dir: follow_dotgit_path(git_dir),
            parent: GlobalGit::default(),
        }
    }

    /// Associate with a working copy.
    pub fn with_working_copy(self, root: PathBuf) -> RepoGit {
        RepoGit {
            root,
            parent: self,
            index_fast_path: false,
        }
    }

    /// The bare repo root, usually ".git" or "<name>.git".
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }
}

impl RepoGit {
    /// Construct from root (parent of ".git") and config.
    pub fn from_root_and_config(root: PathBuf, config: &dyn Config) -> Self {
        let git_dir = root.join(".git");
        Self {
            root,
            parent: BareGit::from_git_dir_and_config(git_dir, config),
            index_fast_path: config
                .get_or_default("experimental", "git-index-fast-path")
                .unwrap_or_default(),
        }
    }

    /// Construct from root (parent of ".git") and default config.
    pub fn from_root(root: PathBuf) -> Self {
        let git_dir = root.join(".git");
        Self {
            root,
            parent: BareGit::from_git_dir(git_dir),
            index_fast_path: false,
        }
    }

    /// The working copy root, without ".git".
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Convert `git diff-index --norenames --raw -z <tree-ish>` output
    /// into `git update-index -z --index-info` input.
    ///
    /// diff-index entry format:
    ///   `:<old_mode> <new_mode> <old_sha> <new_sha> <status>\0<path>\0`
    ///
    /// See https://git-scm.com/docs/git-diff-index#_raw_output_format
    ///
    /// `git update-index --index-info` format:
    ///   `<old_mode> <old_sha> <stage>\t<path>\0`
    ///
    /// See https://git-scm.com/docs/git-update-index#_using_index_info
    fn diff_index_to_index_info(raw: &[u8]) -> io::Result<Vec<u8>> {
        let mut result = Vec::new();
        let mut pos = 0;

        let parse_err = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());

        while pos < raw.len() {
            // Each entry starts with ':'
            if raw[pos] != b':' {
                return Err(parse_err("missing ':' at the beginning"));
            }
            pos += 1;

            // Header ends at the first \0: "<old_mode> <new_mode> <old_sha> <new_sha> <status>"
            let header_end = raw[pos..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| parse_err("no NUL in the entry"))?
                + pos;

            let header = std::str::from_utf8(&raw[pos..header_end])
                .map_err(|e| parse_err(&e.to_string()))?;

            // Split: old_mode, new_mode, old_sha, new_sha, status
            let mut parts = header.splitn(5, ' ');
            let old_mode = parts.next().ok_or_else(|| parse_err("missing old_mode"))?;
            parts.next(); // new_mode
            let old_sha = parts.next().ok_or_else(|| parse_err("missing old_sha"))?;
            parts.next(); // new_sha
            let status = parts.next().ok_or_else(|| parse_err("missing status"))?;

            // Copy (C) and rename (R) should be opted out by the diff-index command.
            // Copied and renamed files show up as addition (A) and deletion (D) instead.
            // Unmerged (U) status should not show up as Sapling does not expose merge conflicts to Git.
            match status {
                "M" | "A" | "D" | "T" => {}
                _ => {
                    return Err(parse_err(&format!(
                        "unexpected diff-index status: {status}"
                    )));
                }
            }

            // Remaining: \0<path>\0
            pos = header_end + 1;
            let path_end = raw[pos..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| parse_err("missing NUL after path"))?
                + pos;
            let path = &raw[pos..path_end];
            pos = path_end + 1;

            // <mode>SP<sha1>SP<stage>TAB<path>
            result.extend_from_slice(old_mode.as_bytes());
            result.push(b' ');
            result.extend_from_slice(old_sha.as_bytes());
            result.extend_from_slice(b" 0\t");
            result.extend_from_slice(path);
            result.push(b'\0');
        }

        Ok(result)
    }

    /// Update git index for mutated paths compared to given commit.
    /// Uses `--index-info` to avoid command-line argument length limits.
    pub fn update_diff_index(&self, treeish: HgId) -> io::Result<ExitStatus> {
        let hex = treeish.to_hex();
        let output = self.call(
            "diff-index",
            &["--cached", "--no-renames", "--raw", "-z", &hex],
        )?;

        let index_info = Self::diff_index_to_index_info(&output.stdout)?;

        let mut cmd = self.git_cmd("update-index", &["-z", "--index-info"]);
        cmd.checked_run_with_stdin(&index_info)
    }
}

impl Deref for BareGit {
    type Target = GlobalGit;

    fn deref(&self) -> &Self::Target {
        &self.parent
    }
}

impl Deref for RepoGit {
    type Target = BareGit;

    fn deref(&self) -> &Self::Target {
        &self.parent
    }
}

impl DerefMut for BareGit {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.parent
    }
}

impl DerefMut for RepoGit {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.parent
    }
}

pub trait GitCmd {
    /// Prepare the `Command` for `git`.
    ///
    /// `cmd_name` is the "main" git command, like "fetch", "bundle crate".
    /// `args` contains the git command arguments.
    /// `opts` provides extra configs, like what is the `git`, verbose, and quiet.
    fn git_cmd(&self, cmd_name: &str, args: &[impl ToString]) -> Command;

    /// Call `git`. Check exit code. Capture output.
    fn call(&self, cmd_name: &str, args: &[impl ToString]) -> io::Result<Output> {
        let mut cmd = self.git_cmd(cmd_name, args);
        cmd.checked_output()
    }

    /// Run `git`. Check exit code.
    fn run(&self, cmd_name: &str, args: &[impl ToString]) -> io::Result<ExitStatus> {
        let mut cmd = self.git_cmd(cmd_name, args);
        cmd.checked_run()
    }
}

impl GitCmd for GlobalGit {
    fn git_cmd(&self, cmd_name: &str, args: &[impl ToString]) -> Command {
        let args = args.iter().map(ToString::to_string).collect();
        git_cmd_impl(cmd_name, args, self, None, None)
    }
}

impl GitCmd for BareGit {
    fn git_cmd(&self, cmd_name: &str, args: &[impl ToString]) -> Command {
        let args = args.iter().map(ToString::to_string).collect();
        git_cmd_impl(cmd_name, args, self, Some(self.git_dir()), None)
    }
}

impl GitCmd for RepoGit {
    fn git_cmd(&self, cmd_name: &str, args: &[impl ToString]) -> Command {
        let args = args.iter().map(ToString::to_string).collect();
        git_cmd_impl(
            cmd_name,
            args,
            self,
            Some(self.git_dir()),
            Some(self.root()),
        )
    }
}

const GIT: &str = "git";

/// Test if a flag is global or not. For cgit, global flags must be positioned
/// before the command name.
fn is_global_flag(arg: &str) -> bool {
    arg == "--no-optional-locks"
}

fn git_cmd_impl(
    cmd_name: &str,
    args: Vec<String>,
    opts: &GlobalGit,
    git_dir: Option<&Path>,
    root: Option<&Path>,
) -> Command {
    let cfg = &opts.config;
    let mut cmd = Command::new(&cfg.git_binary);

    // -c foo.bar=baz ...
    for c in &opts.extra_git_configs {
        cmd.arg("-c");
        cmd.arg(c);
    }

    // --git-dir=...
    if let Some(git_dir) = git_dir {
        cmd.arg(format!("--git-dir={}", git_dir.display()));
        if git_dir.file_name().unwrap_or_default() == ".git" {
            // Run `git` from the repo root. This avoids issues like `git status` being over smart
            // and uses relative paths.
            if let Some(cwd) = root.as_ref() {
                cmd.current_dir(cwd);
            }
        }
    }

    // global flags like --no-optional-locks
    let global_arg_count = args.iter().take_while(|arg| is_global_flag(arg)).count();
    cmd.args(&args[..global_arg_count]);

    // command name, space-separated name like "bundle create" is split to multiple args.
    for arg in cmd_name.split_ascii_whitespace() {
        cmd.arg(arg);
    }

    // insert --verbose and --quiet between the git command name and its arguments
    // not all commands support --verbose or --quiet
    let verbose = cfg.verbose && ["fetch", "push"].contains(&cmd_name);
    if verbose {
        cmd.arg("--verbose");
    }
    let quiet =
        cfg.quiet && ["fetch", "init", "checkout", "push", "bundle create"].contains(&cmd_name);
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.args(&args[global_arg_count..]);

    tracing::debug!("git command: {:?}", &cmd.get_args().collect::<Vec<_>>());

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_index_to_index_info() {
        // Empty input
        assert_eq!(RepoGit::diff_index_to_index_info(b"").unwrap(), b"");

        let raw = b":100644 100644 aaaa bbbb M\0modifiedfile\0\
                     :000000 100644 0000 bbbb A\0addedfile\0\
                     :100755 000000 aaaa 0000 D\0deletedfile\0";
        let out = RepoGit::diff_index_to_index_info(raw).unwrap();
        assert_eq!(
            out,
            b"100644 aaaa 0\tmodifiedfile\0\
              000000 0000 0\taddedfile\0\
              100755 aaaa 0\tdeletedfile\0"
        );
    }

    #[test]
    fn test_diff_index_with_unexpected_status() {
        let raw = b":000000 000000 0000 0000 U\0unmergedfile\0";
        let err = RepoGit::diff_index_to_index_info(raw).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unexpected diff-index status: U"));
    }
}
