// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use std::env;
use std::ffi::OsStr;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use tracing::trace;
use tracing::warn;

/// Builds an SCS client command using any provided overrides, like the scsc
/// binary path and host arguments (e.g. for testing).
pub(crate) fn build_scsc_command() -> Command {
    let scsc_path = env::var_os("SCS_PATH")
        .and_then(|oss| oss.into_string().ok())
        .unwrap_or("scsc".to_string());
    let mb_scs_port = env::var_os("SCS_PORT").and_then(|oss| oss.into_string().ok());

    let mut scsc_cmd = Command::new(&scsc_path);

    scsc_cmd.env("SCSC_WRITES_ENABLED", "1");

    if let Some(scs_port) = mb_scs_port {
        scsc_cmd.arg("--host");
        let host_arg = format!("{0}:{1}", "localhost", scs_port);
        scsc_cmd.arg(host_arg);
    }
    scsc_cmd
}

/// Builds a git command setting up all the necessary configs for authentication.
pub(crate) fn build_git_command() -> Result<Command> {
    let mut git_cmd = Command::new("git");
    if env::var_os("PUSHREBASE_INTEGRATION_TEST").is_some() {
        let tls_ca_path = env::var("THRIFT_TLS_CL_CA_PATH")
            .map_err(|e| anyhow!("failed to get THRIFT_TLS_CL_CA_PATH: {e}"))?;
        let tls_cert_path = env::var("THRIFT_TLS_CL_CERT_PATH")
            .map_err(|e| anyhow!("failed to get THRIFT_TLS_CL_CERT_PATH: {e}"))?;
        let tls_key_path = env::var("THRIFT_TLS_CL_KEY_PATH")
            .map_err(|e| anyhow!("failed to get THRIFT_TLS_CL_KEY_PATH: {e}"))?;

        git_cmd.args([
            "-c",
            &format!("http.sslCAInfo={tls_ca_path}"),
            "-c",
            &format!("http.sslCert={tls_cert_path}"),
            "-c",
            &format!("http.sslKey={tls_key_path}"),
        ]);
    }
    Ok(git_cmd)
}

/// Helper to run a command, printing stderr on failures and parsing and returning
/// stdout on success.
pub(crate) fn run_git_command<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S> + Clone,
    S: AsRef<OsStr> + Into<String>,
{
    let cmd = build_git_command()?;
    run_command(cmd, args, "git", false)
}

pub(crate) fn run_scsc_command<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S> + Clone,
    S: AsRef<OsStr> + Into<String>,
{
    let cmd = build_scsc_command();
    run_command(cmd, args, "scsc", false)
}

/// Helper to run a command, printing stderr on failures and parsing and returning
/// stdout on success.
pub(crate) fn run_command<I, S>(
    mut cmd: Command,
    args: I,
    program: &str,
    quiet: bool, // Don't print any output if we expect command to fail
) -> Result<String>
where
    I: IntoIterator<Item = S> + Clone,
    S: AsRef<OsStr> + Into<String>,
{
    let args_str = args
        .clone()
        .into_iter()
        .map(|arg| arg.into())
        .collect::<Vec<_>>()
        .join(" ");

    cmd.args(args);

    trace!("{program} {args_str} cmd: {0:#?}", &cmd);

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !quiet {
            warn!("{program} {args_str} failed");
            warn!("stderr:\n{}", stderr);
            warn!("stdout:\n{}", stdout);
        }
        return Err(anyhow!(
            "command `{program} {args_str}` failed:\n{}",
            stderr
        ));
    }

    print_cmd_output(&output, &format!("{program} {args_str}"));

    let stdout = String::from_utf8(output.stdout)
        .context("output not valid utf8")?
        .trim()
        .to_string();

    Ok(stdout)
}

/// Helper for debugging the output of commands
pub(crate) fn print_cmd_output(output: &std::process::Output, title: &str) {
    trace!("{title} output:");
    trace!(
        "Stdout: {0:#?}",
        String::from_utf8(output.stdout.clone()).expect("stdout not utf8")
    );

    trace!(
        "Stderr: {0:#?}\n\n",
        String::from_utf8(output.stderr.clone()).expect("stderr not utf8")
    );
}
