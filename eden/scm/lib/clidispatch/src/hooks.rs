/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use erased_serde::Serialize;
use io::IO;
use repo::Repo;

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
    pub(crate) fn run_pre(&self, repo: Option<&Repo>, full_args: &[String]) -> Result<()> {
        run_hooks(&self.pre, repo, full_args, true, None)
    }

    // Run "post-" command hooks, skipping Python hooks. Will not propagate errors from
    // the hooks.
    pub(crate) fn run_post(
        &self,
        repo: Option<&Repo>,
        full_args: &[String],
        result: u8,
    ) -> Result<()> {
        run_hooks(
            &self.post,
            repo,
            full_args,
            false,
            Some(&|kwargs| {
                kwargs.insert("result", Box::new(result));
            }),
        )
    }

    // Run "fail-" command hooks, skipping Python hooks. Will not propagate errors from
    // the hooks.
    pub(crate) fn run_fail(&self, repo: Option<&Repo>, full_args: &[String]) -> Result<()> {
        run_hooks(&self.fail, repo, full_args, false, None)
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

fn run_hooks<'a>(
    hooks: &[hook::Hooks],
    repo: Option<&Repo>,
    full_args: &'a [String],
    propagate_errors: bool,
    extra_kwargs_func: Option<&dyn Fn(&mut BTreeMap<&str, Box<dyn Serialize + 'a>>)>,
) -> Result<()> {
    if hooks.is_empty() {
        return Ok(());
    }

    let mut hook_args = BTreeMap::from([("args", Box::new(full_args) as Box<dyn Serialize>)]);
    if let Some(func) = extra_kwargs_func {
        (func)(&mut hook_args);
    }
    for hooks in hooks {
        hooks.run_hooks(repo, propagate_errors, Some(&hook_args))?;
    }

    Ok(())
}
