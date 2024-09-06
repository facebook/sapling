/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;

use configmodel::Config;
use configmodel::ConfigExt;
use spawn_ext::CommandExt;

use crate::utils::follow_dotgit_path;

/// Run `git` outside a repo.
#[derive(Default, Clone)]
pub struct GlobalGit {
    /// Path to the "git" command.
    pub git_binary: String,

    /// Whether to use --verbose.
    pub verbose: bool,

    /// Whether to use --quiet.
    pub quiet: bool,

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
}

impl GlobalGit {
    /// Construct from config.
    pub fn from_config(config: &dyn Config) -> Self {
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
}

impl BareGit {
    /// Construct from git_dir (".git" path) and config.
    pub fn from_git_dir_and_config(git_dir: PathBuf, config: &dyn Config) -> Self {
        Self {
            git_dir: follow_dotgit_path(git_dir),
            parent: GlobalGit::from_config(config),
        }
    }

    /// Associate with a working copy.
    pub fn with_working_copy(self, root: PathBuf) -> RepoGit {
        RepoGit { root, parent: self }
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
        }
    }

    /// The working copy root, without ".git".
    pub fn root(&self) -> &Path {
        &self.root
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
    let mut cmd = Command::new(&opts.git_binary);

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
    let verbose = opts.verbose && ["fetch", "push"].contains(&cmd_name);
    if verbose {
        cmd.arg("--verbose");
    }
    let quiet =
        opts.quiet && ["fetch", "init", "checkout", "push", "bundle create"].contains(&cmd_name);
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.args(&args[global_arg_count..]);

    tracing::debug!("git command: {:?}", &cmd.get_args().collect::<Vec<_>>());

    cmd
}
