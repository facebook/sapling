/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Weak;

use nodeipc::derive::HasIpc;
use nodeipc::ipc;
use nodeipc::NodeIpc;
use serde::Deserialize;
use serde::Serialize;

use crate::util;

#[derive(Serialize, Deserialize)]
pub struct CommandEnv {
    pub env: Vec<(String, String)>,
    pub cwd: String,
}

#[derive(Serialize, Deserialize)]
pub struct ProcessProps {
    pub pid: u32,
    pub pgid: u32,
    pub groups: Option<Vec<u32>>,
    pub rlimit_nofile: Option<u64>,
}

pub struct Client {
    pub ipc: NodeIpc,
}

pub struct Server<'a> {
    pub ipc: Arc<NodeIpc>,
    pub run_func: &'a (dyn (Fn(&'_ Server<'a>, Vec<String>) -> i32) + Send + Sync),
}

#[ipc]
impl Client {
    /// Run a shell command. Return exit code.
    fn system(&self, env: CommandEnv, command: String) -> i32 {
        tracing::debug!("client::system {}", command);
        let mut cmd = system_command::new_system_command(command);
        let CommandEnv { cwd, env } = env;
        cmd.env_clear().envs(env).current_dir(cwd);
        match cmd.status() {
            Ok(v) => match v.code() {
                Some(v) => v,
                None => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        return match v.signal() {
                            Some(v) => 128 + v,
                            None => todo!(),
                        };
                    }
                    #[allow(unreachable_code)]
                    128
                }
            },
            Err(_) => 127,
        }
    }
}

#[ipc]
impl Server<'_> {
    /// Report the server process environment.
    fn process_props(&self) -> ProcessProps {
        tracing::debug!("server::process_props");
        let pgid = {
            #[cfg(unix)]
            unsafe {
                libc::getpgid(0) as u32
            }
            #[cfg(not(unix))]
            0u32
        };
        ProcessProps {
            pid: std::process::id() as _,
            pgid,
            groups: util::groups(),
            rlimit_nofile: util::rlimit_nofile(),
        }
    }

    /// Apply the environment. Return `true` on success.
    fn apply_env(&self, env: CommandEnv, umask: Option<u32>) -> bool {
        tracing::debug!("server::apply_env");
        let CommandEnv { cwd, env } = env;
        if std::env::set_current_dir(&cwd).is_err() {
            return false;
        }
        let new_key_set: HashSet<_> = env.iter().map(|(k, _)| k).collect();
        for (k, _) in std::env::vars() {
            if !new_key_set.contains(&k) {
                std::env::remove_var(k);
            }
        }
        for (k, v) in &env {
            std::env::set_var(k, v);
        }
        if let Some(umask) = umask {
            #[cfg(unix)]
            unsafe {
                libc::umask(umask as _);
            }
            let _ = umask;
        }
        true
    }

    /// Run the given main command. Return exit code.
    fn run_command(&self, argv: Vec<String>) -> i32 {
        tracing::debug!("server::run_command {:?}", &argv);
        // To avoid circular dependency, we cannot call hgcommands here.
        // Instead, rely on hgcommands to provide Server::run_func.
        (self.run_func)(self, argv)
    }
}

impl Server<'_> {
    /// Get the weak reference of the `NodeIpc` owned by the server.
    /// This is useful if the callsite wants a lifetime-free version of `NodeIpc`
    /// and use it in Python bindings.
    pub fn ipc_weakref(&self) -> Weak<NodeIpc> {
        Arc::downgrade(&self.ipc.clone())
    }
}

impl CommandEnv {
    pub fn current() -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?
            .to_str()
            .ok_or_else(|| anyhow::format_err!("Current directory is not in UTF-8"))?
            .to_owned();
        // Skip NODE_CHANNEL_FD automatically. The other side likely does not want it.
        let env = Self {
            env: std::env::vars()
                .filter(|(k, _)| k != "NODE_CHANNEL_FD")
                .collect(),
            cwd,
        };
        Ok(env)
    }
}

impl HasIpc for Client {
    fn ipc(&self) -> &NodeIpc {
        &self.ipc
    }
}

impl HasIpc for Server<'_> {
    fn ipc(&self) -> &NodeIpc {
        &self.ipc
    }
}
