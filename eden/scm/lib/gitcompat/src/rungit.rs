/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::process::Command;

use configmodel::Config;
use configmodel::ConfigExt;

/// Options used by `run_git`.
#[derive(Default, Clone)]
pub struct RunGitOptions {
    /// Path to the "git" command.
    pub git_binary: String,

    /// Whether to use --verbose.
    pub verbose: bool,

    /// Whether to use --quiet.
    pub quiet: bool,

    /// The `GIT_DIR`.
    pub git_dir: Option<PathBuf>,

    /// Extra Git configs, "foo.bar=baz".
    pub extra_git_configs: Vec<String>,
}

impl RunGitOptions {
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

    /// Prepare the `Command` for `git`.
    ///
    /// `cmd_name` is the "main" git command, like "fetch", "bundle crate".
    /// `args` contains the git command arguments.
    /// `opts` provides extra configs, like what is the `git`, verbose, and quiet.
    pub fn git_cmd(&self, cmd_name: &str, args: &[impl ToString]) -> Command {
        let args = args.iter().map(ToString::to_string).collect();
        git_cmd_impl(cmd_name, args, self)
    }
}

const GIT: &str = "git";

fn git_cmd_impl(cmd_name: &str, args: Vec<String>, opts: &RunGitOptions) -> Command {
    let mut cmd = Command::new(&opts.git_binary);

    // -c foo.bar=baz ...
    for c in &opts.extra_git_configs {
        cmd.arg("-c");
        cmd.arg(c);
    }

    // --git-dir=...
    if let Some(git_dir) = &opts.git_dir {
        cmd.arg(format!("--git-dir={}", git_dir.display()));
    }

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
        cmd.arg("--verbose");
    }
    cmd.args(args);

    cmd
}
