// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

//! A library for running shell commands with additional functionality.
//!
//! This library provides a wrapper around `std::process::Command` that adds
//! additional functionality for running shell commands with custom environment
//! variables, working directories, and more.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::process::Command;
use std::process::Output;

use anyhow::Context;
use anyhow::Result;
use tracing::debug;
use tracing::info;
use tracing::instrument;
use tracing::warn;

/// A wrapper around `std::process::Command` that provides additional functionality
/// for running shell commands with custom environment variables.
pub struct ShellCommand {
    /// The command to run
    program: String,

    /// The arguments to pass to the command
    args: Vec<String>,

    /// Additional environment variables to set
    env_vars: HashMap<String, String>,

    /// Whether to inherit the parent process's environment
    inherit_env: bool,

    /// The working directory to run the command in
    current_dir: Option<String>,
}

impl ShellCommand {
    /// Create a new ShellCommand
    pub fn new(program: &str) -> Self {
        ShellCommand {
            program: program.to_string(),
            args: Vec::new(),
            env_vars: HashMap::new(),
            inherit_env: true,
            current_dir: None,
        }
    }

    /// Add an argument to the command
    pub fn arg<S: AsRef<str>>(mut self, arg: S) -> Self {
        self.args.push(arg.as_ref().to_string());
        self
    }

    /// Add multiple arguments to the command
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for arg in args {
            self.args.push(arg.as_ref().to_string());
        }
        self
    }

    /// Set an environment variable for the command
    pub fn env<K, V>(mut self, key: K, val: V) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        self.env_vars
            .insert(key.as_ref().to_string(), val.as_ref().to_string());
        self
    }

    /// Set multiple environment variables for the command
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (key, val) in vars {
            self.env_vars
                .insert(key.as_ref().to_string(), val.as_ref().to_string());
        }
        self
    }

    /// Set whether to inherit the parent process's environment
    pub fn inherit_env(mut self, inherit: bool) -> Self {
        self.inherit_env = inherit;
        self
    }

    /// Set the working directory for the command
    pub fn current_dir<S: AsRef<str>>(mut self, dir: S) -> Self {
        self.current_dir = Some(dir.as_ref().to_string());
        self
    }

    /// Run the command and return the output
    #[instrument(skip(self))]
    pub fn run(&self) -> Result<Output> {
        let cmd_str = self.to_string();
        info!(command = %cmd_str, "Running command");

        let mut command = Command::new(&self.program);
        command.args(&self.args);

        // Set environment variables
        if !self.inherit_env {
            command.env_clear();
        }
        for (key, val) in &self.env_vars {
            command.env(key, val);
            debug!(key = key, value = val, "Setting environment variable");
        }
        // Set working directory if specified
        if let Some(dir) = &self.current_dir {
            command.current_dir(dir);
            debug!(directory = dir, "Setting working directory");
        }
        // Run the command
        let output = command
            .output()
            .with_context(|| format!("Failed to execute command: {}", cmd_str))?;

        // Log the result
        if output.status.success() {
            debug!(status = %output.status, "Command succeeded");
        } else {
            warn!(status = %output.status, "Command failed");
            if !output.stderr.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(stderr = %stderr, "Command stderr");
            }
        }
        Ok(output)
    }

    /// Get the stdout as a string
    pub fn stdout_str(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get the stderr as a string
    pub fn stderr_str(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    }

    /// Run the command and check that it succeeded
    #[instrument(skip(self))]
    pub fn run_success(&self) -> Result<Output> {
        let output = self.run()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Command failed with status {}: {} (stderr: {}, stdout: {})",
                output.status,
                self.to_string(),
                Self::stderr_str(&output),
                Self::stdout_str(&output)
            ));
        }
        Ok(output)
    }
}

/// Get the command as a string for logging
impl std::fmt::Display for ShellCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = self.program.clone();

        for arg in &self.args {
            result.push(' ');
            // Quote arguments with spaces
            if arg.contains(' ') {
                result.push('"');
                result.push_str(arg);
                result.push('"');
            } else {
                result.push_str(arg);
            }
        }

        write!(f, "{}", result)
    }
}

/// Run a simple command with the given arguments and return the output
#[instrument]
pub fn run_command<I, S>(program: &str, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    I: Debug,
    S: AsRef<OsStr>,
{
    let cmd_str = format!("{} {:?}", program, args);
    info!(command = %cmd_str, "Running command");

    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute command: {}", cmd_str))?;

    if output.status.success() {
        debug!(status = %output.status, "Command succeeded");
    } else {
        warn!(status = %output.status, "Command failed");

        if !output.stderr.is_empty() {
            let stderr = ShellCommand::stderr_str(&output);
            warn!(stderr = %stderr, "Command stderr");
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use tracing::subscriber::with_default;
    use tracing_subscriber::FmtSubscriber;

    use super::*;

    fn setup_tracing() {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _ = with_default(subscriber, || {});
    }

    #[test]
    fn test_shell_command_basic() {
        setup_tracing();

        // Test a simple echo command
        let output = ShellCommand::new("echo")
            .arg("hello")
            .run_success()
            .unwrap();

        let stdout = ShellCommand::stdout_str(&output);
        assert_eq!(stdout, "hello");
    }

    #[test]
    fn test_shell_command_env() {
        setup_tracing();

        // Test setting an environment variable
        let output = ShellCommand::new("sh")
            .arg("-c")
            .arg("echo $TEST_VAR")
            .env("TEST_VAR", "test_value")
            .run_success()
            .unwrap();

        let stdout = ShellCommand::stdout_str(&output);
        assert_eq!(stdout, "test_value");
    }
}
