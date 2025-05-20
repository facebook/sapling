/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;
use std::process::Command;
use std::process::ExitStatus;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use configmodel::Config;
use configmodel::ConfigExt;
use erased_serde::Serialize;
use io::IO;
use io::IsTty;
use minibytes::Text;
use repo::Repo;
use serde_json::Value;
use spawn_ext::CommandExt;

#[derive(Debug)]
pub struct Hook {
    name: Text,
    priority: i64,
    ignored: bool,
    spec: HookSpec,
}

impl Hook {
    pub fn is_python(&self) -> bool {
        matches!(self.spec, HookSpec::Python(_))
    }
}

#[derive(Debug, PartialEq)]
enum HookSpec {
    Shell { script: Text, background: bool },
    Python(Text),
}

pub struct Hooks {
    io: IO,
    hook_type: String,
    hooks: Vec<Hook>,
    verbose: bool,
}

pub struct PythonHookSig;

impl<'a> factory::FunctionSignature<'a> for PythonHookSig {
    /// (repo, spec, hook_name, kwargs)
    /// See run_python_hook in pyhook.
    type In = (
        Option<&'a Repo>,
        &'a str,
        &'a str,
        Option<&'a dyn Serialize>,
    );
    type Out = Result<i8>;
}

impl Hooks {
    pub fn from_config(cfg: &dyn Config, io: &IO, hook_type: &str) -> Self {
        let verbose = cfg.get_or_default("ui", "verbose").unwrap_or_default()
            || cfg.get_or_default("ui", "debug").unwrap_or_default();
        Self {
            io: io.clone(),
            hook_type: hook_type.to_string(),
            hooks: hooks_from_config(cfg, hook_type),
            verbose,
        }
    }

    /// Ignore hooks with the exact definition.
    ///
    /// lib/checkout uses this to skip hooks like
    /// "hooks.edenfs-update.eden-redirect=python:sapling.hooks.edenfs_redirect_fixup"
    /// because it has the same behavior implemented natively.
    pub fn ignore(&mut self, spec: &'static str) {
        let text = Text::from_static(spec);
        let spec = HookSpec::from_text(text);
        for hook in &mut self.hooks {
            if hook.spec == spec {
                hook.ignored = true;
            }
        }
    }

    /// Report python hook names that won't run by `run_hooks`.
    pub fn python_hook_names(&self) -> Vec<Text> {
        self.hooks
            .iter()
            .filter(|h| h.is_python() && h.ignored)
            .map(|h| h.name.clone())
            .collect()
    }

    pub fn run_hooks(
        &self,
        repo: Option<&Repo>,
        propagate_errors: bool,
        kwargs: Option<&dyn Serialize>,
    ) -> Result<()> {
        let client_info = clientinfo::get_client_request_info();

        // Set `SL_LOG=commands::run::blocked=debug` to see the blocked duration.
        // By default, duration < 10ms is ignored.
        let _blocked = self
            .io
            .time_interval()
            .scoped_blocked_interval("hook".into());

        for h in self.hooks.iter() {
            if h.ignored {
                tracing::debug!(hook=%h.name, "hook ignored");
                continue;
            }

            if self.verbose {
                let _ = write!(self.io.error(), "calling hook: {}\n", h.name.as_ref());
            }

            let span = tracing::info_span!("hook", hook = %h.name, exit = tracing::field::Empty);
            let _enter = span.enter();

            let maybe_error_description = match &h.spec {
                HookSpec::Shell {
                    script: shell_cmd,
                    background,
                } => 'shell_hook: {
                    let mut cmd = Command::new_shell(shell_cmd);

                    if let Some(repo) = repo {
                        cmd.current_dir(repo.path());
                    }

                    cmd.env("HG_HOOKTYPE", &self.hook_type);
                    cmd.env("HG_HOOKNAME", h.name.as_ref());
                    cmd.env(
                        "SAPLING_CLIENT_ENTRY_POINT",
                        format!("{}", client_info.entry_point),
                    );
                    cmd.env("SAPLING_CLIENT_CORRELATOR", &client_info.correlator);

                    if let Some(kwargs) = kwargs {
                        for (k, v) in to_env_vars(kwargs)? {
                            match v {
                                Some(v) => cmd.env(k, v),
                                None => cmd.env_remove(k),
                            };
                        }
                    }

                    cmd.env(
                        "HG",
                        std::env::current_exe()
                            .ok()
                            .as_ref()
                            .and_then(|p| p.to_str())
                            .unwrap_or_else(|| identity::cli_name()),
                    );

                    if *background {
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

                        if !status.success() {
                            let exit_description = exit_description(&status);
                            break 'shell_hook Some(exit_description);
                        }
                    }
                    None
                }
                HookSpec::Python(spec) => {
                    match run_python_hook(repo, spec.as_ref(), h.name.as_ref(), kwargs) {
                        Err(e) => Some(format!("{} hook error: {}", h.name, e)),
                        Ok(0) => None,
                        Ok(v) => Some(format!("{} hook returned non-zero: {}", h.name, v)),
                    }
                }
            };

            match maybe_error_description {
                None => {
                    span.record("exit", "success");
                }
                Some(e) => {
                    span.record("exit", &e);
                    match propagate_errors {
                        false => tracing::warn!(exit=e, hook=%h.name, "bad hook exit"),
                        true => bail!("{} hook {}", h.name, e),
                    }
                }
            }
        }

        Ok(())
    }
}

/// Converts a `dyn Serialize` to env vars used by shell hooks.
fn to_env_vars(
    kwargs: &dyn Serialize,
) -> Result<impl IntoIterator<Item = (String, Option<String>)> + use<>> {
    let args = serde_json::to_value(kwargs)?;
    let map = match args {
        Value::Object(map) => map,
        _ => bail!("shell hook args is not a map: {}", args),
    };
    Ok(map.into_iter().map(|(k, v)| {
        let env_name = format!("HG_{}", k.to_uppercase());
        let env_value = match v {
            Value::Null => return (env_name, None),
            Value::String(v) => v, // do not quote the string
            Value::Array(v) if v.iter().all(|i| i.is_string()) => {
                // Shell escape a list of strings.
                let args: Vec<&str> = v.iter().filter_map(|i| i.as_str()).collect();
                sysutil::shell_escape(&args)
            }
            _ => v.to_string(),
        };
        (env_name, Some(env_value))
    }))
}

fn run_python_hook(
    repo: Option<&Repo>,
    hook_spec: &str,
    hook_name: &str,
    kwargs: Option<&dyn Serialize>,
) -> Result<i8> {
    match factory::call_function::<PythonHookSig>((repo, hook_spec, hook_name, kwargs)) {
        Some(v) => v,
        None => bail!("Python hooks are not enabled at runtime"),
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

impl HookSpec {
    fn from_text(value: Text) -> Self {
        if value.starts_with(PY_PREFIX) {
            HookSpec::Python(value.slice(PY_PREFIX.len()..))
        } else if value.starts_with(BG_PREFIX) {
            HookSpec::Shell {
                script: value.slice(BG_PREFIX.len()..),
                background: true,
            }
        } else {
            HookSpec::Shell {
                script: value,
                background: false,
            }
        }
    }
}

fn hooks_from_config(cfg: &dyn Config, hook_name_prefix: &str) -> Vec<Hook> {
    let mut hooks = Vec::new();

    if hook_name_prefix == "priority" || hook_name_prefix == "disabled" {
        return hooks;
    }

    // Python hooks not running through pyhook are incompatible.
    // This is temporary. Remove once pyhook runs fine in production.
    let pyhook_enabled = cfg
        .get_or_default::<bool>("experimental", "run-python-hooks-via-pyhook")
        .unwrap_or_default();

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

        // Add a way to disable hooks.
        let disabled = cfg
            .must_get("hooks", &format!("disabled.{name}"))
            .unwrap_or_default();

        let spec = HookSpec::from_text(value);
        let ignored = disabled || (matches!(spec, HookSpec::Python(..)) && !pyhook_enabled);

        hooks.push(Hook {
            name,
            priority,
            ignored,
            spec,
        });
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
        assert_eq!(hooks[0].spec, HookSpec::Python("foo.py".into()));

        assert_eq!(hooks[1].name, "foo");
        assert_eq!(
            hooks[1].spec,
            HookSpec::Shell {
                script: "echo ok".into(),
                background: false,
            }
        );

        assert_eq!(hooks[2].name, "foo.bar");
        assert_eq!(
            hooks[2].spec,
            HookSpec::Shell {
                script: "touch foo".into(),
                background: true,
            }
        );
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

        let mut hooks = Hooks::from_config(&cfg, &io, "foo");
        assert_eq!(hooks.python_hook_names(), &["foo.python"]);

        // Cannot run the Python hook from this test.
        hooks.ignore("python:foo.py");
        assert!(hooks.run_hooks(None, true, None).is_ok());
        assert!(String::from_utf8(output.to_vec())?.starts_with("ok"));

        // Now with an erroring hook propagating error:

        let cfg = BTreeMap::from([("hooks.foo.shell", "not-a-real-command")]);

        let mut hooks = Hooks::from_config(&cfg, &io, "foo");
        assert!(hooks.run_hooks(None, true, None).is_err());

        // Now not propagating error:

        assert!(hooks.run_hooks(None, false, None).is_ok());

        // Hooks can be ignored.
        for spec in cfg.values() {
            hooks.ignore(spec)
        }
        assert!(hooks.run_hooks(None, true, None).is_ok());

        Ok(())
    }

    #[test]
    fn test_disable_hooks_via_config() {
        let mut cfg = BTreeMap::from([
            ("hooks.foo.hook1", "true"),
            ("hooks.foo.hook2", "python:a.b:c"),
            ("experimental.run-python-hooks-via-pyhook", "true"),
        ]);

        let hooks = hooks_from_config(&cfg, "foo");
        assert!(!hooks[0].ignored);
        assert!(!hooks[1].ignored);

        // Disable via "disabled.".
        cfg.insert("hooks.disabled.foo.hook1", "true");
        cfg.insert("hooks.disabled.foo.hook2", "true");
        let hooks = hooks_from_config(&cfg, "foo");
        assert!(hooks[0].ignored);
        assert!(hooks[1].ignored);

        // Enable via "disabled.".
        cfg.insert("hooks.disabled.foo.hook1", "false");
        cfg.insert("hooks.disabled.foo.hook2", "false");
        let hooks = hooks_from_config(&cfg, "foo");
        assert!(!hooks[0].ignored);
        assert!(!hooks[1].ignored);

        // Disable pyhooks.
        cfg.insert("experimental.run-python-hooks-via-pyhook", "false");
        let hooks = hooks_from_config(&cfg, "foo");
        assert!(!hooks[0].ignored);
        assert!(hooks[1].ignored);
    }

    #[test]
    fn test_shell_env_vars() {
        let t = |m: &dyn Serialize| -> Vec<String> {
            to_env_vars(m)
                .unwrap()
                .into_iter()
                .map(|(k, v)| {
                    let v = match &v {
                        Some(v) => v.as_str(),
                        None => "(unset)",
                    };
                    format!("{}={}", k, v)
                })
                .collect()
        };
        // Strings
        assert_eq!(
            t(&BTreeMap::from([("a", "1"), ("b", "2")])),
            ["HG_A=1", "HG_B=2"]
        );
        // Numbers
        assert_eq!(
            t(&BTreeMap::from([("a", 1i32), ("b", 2)])),
            ["HG_A=1", "HG_B=2"]
        );
        // `None` can be used to unset env vars.
        assert_eq!(
            t(&BTreeMap::from([("a", Some("1")), ("b", None)])),
            ["HG_A=1", "HG_B=(unset)"]
        );
        // Arrays of strings will be shell-escaped.
        #[cfg(unix)]
        assert_eq!(
            t(&BTreeMap::from([("a", vec!["1", "2 3"]), ("b", vec![])])),
            ["HG_A=1 '2 3'", "HG_B="]
        );
        // Arrays of numbers will be JSON encoded.
        assert_eq!(
            t(&BTreeMap::from([("a", vec![1, 2]), ("b", vec![])])),
            ["HG_A=[1,2]", "HG_B="]
        );
    }
}
