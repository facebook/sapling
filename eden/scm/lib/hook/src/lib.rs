/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IsTty;
use io::IO;
use minibytes::Text;
use spawn_ext::CommandExt;

#[derive(Debug)]
pub struct Hook {
    name: Text,
    priority: i64,
    background: bool,
    typ: HookType,
}

impl Hook {
    pub fn is_python(&self) -> bool {
        matches!(self.typ, HookType::Python(_))
    }
}

#[derive(Debug, PartialEq)]
enum HookType {
    Shell(Text),
    Python(Text),
}

pub struct Hooks {
    io: IO,
    hook_type: String,
    hooks: Vec<Hook>,
}

impl Hooks {
    pub fn from_config(cfg: &dyn Config, io: &IO, hook_type: &str) -> Self {
        Self {
            io: io.clone(),
            hook_type: hook_type.to_string(),
            hooks: hooks_from_config(cfg, hook_type),
        }
    }

    pub fn python_hook_names(&self) -> Vec<Text> {
        self.hooks
            .iter()
            .filter(|h| h.is_python())
            .map(|h| h.name.clone())
            .collect()
    }

    pub fn run_shell_hooks(
        &self,
        repo_root: Option<&Path>,
        propagate_errors: bool,
        args: &HashMap<String, String>,
    ) -> Result<()> {
        let client_info = clientinfo::get_client_request_info();

        for h in self.hooks.iter() {
            if let HookType::Shell(shell_cmd) = &h.typ {
                let span =
                    tracing::info_span!("shell hook", hook = %h.name, exit = tracing::field::Empty);
                let _enter = span.enter();

                let mut cmd = Command::new_shell(shell_cmd);

                if let Some(repo_root) = &repo_root {
                    cmd.current_dir(repo_root);
                }

                cmd.env("HG_HOOKTYPE", &self.hook_type);
                cmd.env("HG_HOOKNAME", h.name.as_ref());
                cmd.env(
                    "SAPLING_CLIENT_ENTRY_POINT",
                    &format!("{}", client_info.entry_point),
                );
                cmd.env("SAPLING_CLIENT_CORRELATOR", &client_info.correlator);

                for (k, v) in args {
                    cmd.env(format!("HG_{}", k.to_uppercase()), v);
                }

                cmd.env(
                    "HG",
                    std::env::current_exe()
                        .ok()
                        .as_ref()
                        .and_then(|p| p.to_str())
                        .unwrap_or_else(|| identity::cli_name()),
                );

                if h.background {
                    if let Err(err) = cmd.spawn_detached() {
                        tracing::warn!(?err, "error spawning background hook");
                    }
                } else {
                    let _hook_blocked = self
                        .io
                        .time_interval()
                        .scoped_blocked_interval("exthook".into());

                    let status = if self.io.output().is_tty() {
                        // If stdout is tty, let child inherit our file handles.
                        cmd.status()
                    } else {
                        // Stdout is not a tty - capture output and proxy to our IO.
                        // Stdin is not inherited.
                        match cmd.output() {
                            Err(err) => Err(err),
                            Ok(out) => {
                                let _ = self.io.output().write_all(&out.stdout);
                                let _ = self.io.error().write_all(&out.stderr);
                                Ok(out.status)
                            }
                        }
                    }
                    .with_context(|| format!("starting hook {}", h.name))?;

                    if status.success() {
                        span.record("exit", "success");
                    } else {
                        let exit_description = exit_description(&status);
                        span.record("exit", &exit_description);

                        if propagate_errors {
                            bail!("{} hook {}", h.name, exit_description);
                        } else {
                            tracing::warn!(exit=exit_description, hook=%h.name, "bad hook exit");
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

fn exit_description(status: &ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("killed by signal {signal}");
        }
    }

    match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "exited with no status or signal (?)".to_string(),
    }
}

const PY_PREFIX: &str = "python:";
const BG_PREFIX: &str = "background:";

fn hooks_from_config(cfg: &dyn Config, hook_name_prefix: &str) -> Vec<Hook> {
    let mut hooks = Vec::new();

    for name in cfg.keys("hooks") {
        if name != hook_name_prefix && !name.starts_with(&format!("{hook_name_prefix}.")) {
            continue;
        }

        let Some(value) = cfg.get_nonempty("hooks", &name) else {
            continue;
        };

        let priority: i64 = cfg
            .must_get("hooks", &format!("priority.{name}"))
            .unwrap_or_default();

        if value.starts_with(PY_PREFIX) {
            hooks.push(Hook {
                name,
                priority,
                background: false,
                typ: HookType::Python(value.slice(PY_PREFIX.len()..)),
            });
        } else if value.starts_with(BG_PREFIX) {
            hooks.push(Hook {
                name,
                priority,
                background: true,
                typ: HookType::Shell(value.slice(BG_PREFIX.len()..)),
            });
        } else {
            hooks.push(Hook {
                name,
                priority,
                background: false,
                typ: HookType::Shell(value),
            });
        }
    }

    hooks.sort_by_key(|h| -h.priority);

    hooks
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use io::BufIO;

    use super::*;

    #[test]
    fn test_hooks_from_config() {
        let cfg = BTreeMap::from([
            ("hooks.foo", "echo ok"),
            ("hooks.foo.bar", "background:touch foo"),
            ("hooks.foo.baz.qux", "python:foo.py"),
            ("hooks.priority.foo.baz.qux", "1"),
            ("hooks.foobar", "echo no"),
        ]);

        let hooks = hooks_from_config(&cfg, "foo");

        assert_eq!(hooks.len(), 3);

        assert_eq!(hooks[0].name, "foo.baz.qux");
        assert!(!hooks[0].background);
        assert_eq!(hooks[0].typ, HookType::Python("foo.py".into()));

        assert_eq!(hooks[1].name, "foo");
        assert!(!hooks[1].background);
        assert_eq!(hooks[1].typ, HookType::Shell("echo ok".into()));

        assert_eq!(hooks[2].name, "foo.bar");
        assert!(hooks[2].background);
        assert_eq!(hooks[2].typ, HookType::Shell("touch foo".into()));
    }

    #[test]
    fn test_run_hooks() -> Result<()> {
        let cfg = BTreeMap::from([
            ("hooks.foo.shell", "echo ok"),
            (
                "hooks.foo.background",
                "background:wont-work-but-doesnt-matter",
            ),
            ("hooks.foo.python", "python:foo.py"),
        ]);

        let output = BufIO::empty();
        let io = IO::new(BufIO::dev_null(), output.clone(), None::<BufIO>);

        let hooks = Hooks::from_config(&cfg, &io, "foo");
        assert_eq!(hooks.python_hook_names(), &["foo.python"]);
        assert!(hooks.run_shell_hooks(None, true, &HashMap::new()).is_ok());
        assert!(String::from_utf8(output.to_vec())?.starts_with("ok"));

        // Now with an erroring hook propagating error:

        let cfg = BTreeMap::from([("hooks.foo.shell", "not-a-real-command")]);

        let hooks = Hooks::from_config(&cfg, &io, "foo");
        assert!(hooks.run_shell_hooks(None, true, &HashMap::new()).is_err());

        // Now not propagating error:

        assert!(hooks.run_shell_hooks(None, false, &HashMap::new()).is_ok());

        Ok(())
    }
}
