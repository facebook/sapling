/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;

use crate::command::CommandDefinition;
use crate::fallback;

pub(crate) struct Hooks {
    io: IO,
    pre: Vec<hook::Hooks>,
    post: Vec<hook::Hooks>,
    fail: Vec<hook::Hooks>,
}

impl Hooks {
    // Initialize pre, post, and fail hooks for this command.
    // May raise FallbackToPython error if Python hooks are configured.
    pub(crate) fn new(config: &dyn Config, io: &IO, command: &CommandDefinition) -> Result<Self> {
        let mut val = Self {
            io: io.clone(),
            pre: Vec::new(),
            post: Vec::new(),
            fail: Vec::new(),
        };

        // Escape hatch in case Rust hooks cause unepxected problems.
        if !config.get_or("experimental", "enable-rust-hooks", || true)? {
            return Ok(val);
        }

        val.add_command(command.main_alias(), config)?;

        // If we rename a command, run hooks named using either old or new name.
        if let Some(legacy_name) = command.legacy_alias() {
            val.add_command(legacy_name, config)?;
        }

        Ok(val)
    }

    // Run "pre-" command hooks, skipping Python hooks. Will propagate errors from the
    // hooks, aborting the command.
    pub(crate) fn run_pre(&self, repo_root: Option<&Path>, full_args: &[String]) -> Result<()> {
        if self.pre.is_empty() {
            return Ok(());
        }

        let full_args = util::sys::shell_escape(full_args);
        let hook_args = HashMap::from([("args".to_string(), full_args)]);
        for hooks in &self.pre {
            hooks.run_shell_hooks(repo_root, true, &hook_args)?;
        }

        Ok(())
    }

    // Run "post-" command hooks, skipping Python hooks. Will not propagate errors from
    // the hooks.
    pub(crate) fn run_post(
        &self,
        repo_root: Option<&Path>,
        full_args: &[String],
        result: u8,
    ) -> Result<()> {
        if self.post.is_empty() {
            return Ok(());
        }

        let full_args = util::sys::shell_escape(full_args);
        let hook_args = HashMap::from([
            ("args".to_string(), full_args),
            ("result".to_string(), format!("{result}")),
        ]);
        for hooks in &self.post {
            hooks.run_shell_hooks(repo_root, false, &hook_args)?;
        }

        Ok(())
    }

    // Run "fail-" command hooks, skipping Python hooks. Will not propagate errors from
    // the hooks.
    pub(crate) fn run_fail(&self, repo_root: Option<&Path>, full_args: &[String]) -> Result<()> {
        if self.fail.is_empty() {
            return Ok(());
        }

        let full_args = util::sys::shell_escape(full_args);
        let hook_args = HashMap::from([("args".to_string(), full_args)]);
        for hooks in &self.fail {
            hooks.run_shell_hooks(repo_root, false, &hook_args)?;
        }

        Ok(())
    }

    fn add_command(&mut self, command_name: &str, config: &dyn Config) -> Result<()> {
        self.pre
            .push(self.load_hooks(config, command_name, &format!("pre-{command_name}"))?);
        self.post
            .push(self.load_hooks(config, command_name, &format!("post-{command_name}"))?);
        self.fail
            .push(self.load_hooks(config, command_name, &format!("fail-{command_name}"))?);
        Ok(())
    }

    fn load_hooks(
        &self,
        config: &dyn Config,
        command: &str,
        hook_type: &str,
    ) -> Result<hook::Hooks> {
        // Hacky, but we don't really have a way to know if a Rust command can fall back
        // to Python. Some Rust commands don't exist in Python or only have legacy
        // implementations in Python.
        const CAN_FALLBACK_TO_PYTHON: &[&str] = &["goto", "update", "status"];

        let hooks = hook::Hooks::from_config(config, &self.io, hook_type);
        let python_names = hooks.python_hook_names();

        // We don't support running Python hooks yet from Rust. If we have Python hooks,
        // we need to either fall back to Python, or warn that we aren't running the
        // hooks.
        if !python_names.is_empty() {
            if CAN_FALLBACK_TO_PYTHON.iter().any(|c| *c == command) {
                fallback!("python hooks");
            } else {
                let _ = writeln!(
                    self.io.error(),
                    "WARNING: not running python hooks {:?}",
                    python_names
                );
            }
        }

        Ok(hooks)
    }
}
