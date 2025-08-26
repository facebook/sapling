/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use edenfs_commands::is_command_enabled_in_rust;
#[cfg(fbcode_build)]
use edenfs_telemetry::EDENFSCTL_CLI_USAGE;
#[cfg(fbcode_build)]
use edenfs_telemetry::cli_usage::CliUsageSample;
#[cfg(fbcode_build)]
use edenfs_telemetry::send;
#[cfg(windows)]
use edenfs_utils::execute_par;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use fail::fail_point;
use fbinit::FacebookInit;
use testutil::failpoint;
use tracing_subscriber::filter::EnvFilter;

#[cfg(not(fbcode_build))]
// For non-fbcode builds, CliUsageSample is not defined. Let's give it a dummy
// value so we can pass CliUsageSample through wrapper_main() and fallback().
struct CliUsageSample;

/// Value used in Python to indicate a command failed to parse
pub const PYTHON_EDENFSCTL_EX_USAGE: i32 = 64;

fn python_fallback() -> Result<Command> {
    if let Ok(args) = std::env::var("EDENFSCTL_REAL") {
        // We might get a command starting with python.exe here instead of a simple path.
        let mut parts = args.split_ascii_whitespace();
        let binary = parts
            .next()
            .ok_or_else(|| anyhow!("invalid fallback environment variable: {:?}", args))?;
        let mut cmd = Command::new(binary);
        #[cfg(windows)]
        if binary.ends_with(".par") {
            cmd = execute_par(binary.into())?;
        }
        cmd.args(parts);
        tracing::debug!("Using binary set by EDENFSCTL_REAL {:?}", &cmd);
        return Ok(cmd);
    }

    let binary = std::env::current_exe().context("unable to retrieve path to the executable")?;
    let binary =
        std::fs::canonicalize(binary).context("unable to canonicalize path to the executable")?;
    #[cfg(windows)]
    let binary = strip_unc_prefix(binary);
    let libexec = binary.parent().ok_or_else(|| {
        anyhow!(
            "unable to retrieve parent directory to '{}'",
            binary.display()
        )
    })?;

    let executable = libexec.join(if cfg!(windows) {
        "edenfsctl.real.exe"
    } else {
        "edenfsctl.real"
    });
    tracing::debug!("trying {:?}", executable);
    if executable.exists() {
        return Ok(Command::new(executable));
    }

    // On Windows we are shipping the Python edenfsctl as PAR file that is not executable by itself
    #[cfg(windows)]
    {
        let par = libexec.join("edenfsctl.real.par");
        tracing::debug!("trying {:?}", par);

        if par.exists() {
            return execute_par(par);
        }
    }

    Err(anyhow!("unable to locate fallback binary"))
}

fn fallback(reason: Option<&clap::Error>) -> Result<i32> {
    if std::env::var("EDENFS_LOG").is_ok() {
        setup_logging();
    }

    if let Some(reason) = reason {
        tracing::debug!(%reason, "falling back to Python");
    }

    let mut cmd = python_fallback()?;
    // skip arg0
    cmd.args(std::env::args().skip(1));

    // Users have PYTHONHOME and PYTHONPATH variables
    // that break the python version of edenfsctl since it will fail to
    // import modules. So, let's strip the PYTHONHOME and PYTHONPATH variables.
    cmd.env_remove("PYTHONHOME");
    cmd.env_remove("PYTHONPATH");

    tracing::debug!("Falling back to {:?}", cmd);

    // Create a subprocess to run Python edenfsctl
    let status = cmd
        .status()
        .with_context(|| format!("failed to execute: {:?}", cmd))?;

    Ok(status.code().unwrap_or(1))
}

/// Setup tracing logging. If we are in development mode, we use the fancier logger, otherwise a
/// simple logger for production use. Logs will be printined to stderr when `--debug` flag is
/// passed.
fn setup_logging() {
    let subscriber = tracing_subscriber::fmt();
    #[cfg(debug_assertions)]
    let subscriber = subscriber.pretty();
    let subscriber = subscriber.with_env_filter(EnvFilter::from_env("EDENFS_LOG"));

    if let Err(e) = subscriber.try_init() {
        eprintln!(
            "Unable to initialize logger. Logging will be disabled. Cause: {:?}",
            e
        );
    }
}

fn rust_main(cmd: edenfs_commands::MainCommand) -> Result<i32> {
    if cmd.debug {
        setup_logging();
    }
    cmd.run()
}

/// This function takes care of the fallback logic, hijack supported subcommand
/// to Rust implementation and forward the rest to Python.
#[allow(unused_variables)]
fn wrapper_main(telemetry_sample: &mut CliUsageSample) -> Result<i32> {
    if std::env::var("EDENFSCTL_ONLY_RUST").is_ok() {
        let cmd = edenfs_commands::MainCommand::try_parse();
        match cmd {
            Ok(cmd) => {
                #[cfg(fbcode_build)]
                {
                    // mark the command is triggered as a Rust command
                    telemetry_sample.set_rust_command(true);
                }
                rust_main(cmd)
            }
            // We failed to parse the command. We should exit with the same
            // exit code that Python exits with for parse failures.
            Err(e) if e.kind() == clap::ErrorKind::UnknownArgument => {
                std::process::exit(PYTHON_EDENFSCTL_EX_USAGE)
            }
            // Some other error occurred during parsing. Let's exit like normal
            // since we can't confirm it was due to an invalid command/arg.
            Err(e) => e.exit(),
        }
    } else if std::env::var("EDENFSCTL_SKIP_RUST").is_ok() {
        fallback(None)
    } else {
        match edenfs_commands::MainCommand::try_parse() {
            // The command is defined in Rust, but check whether it's "enabled"
            // for Rust or else fall back to Python.
            Ok(cmd) => {
                if cmd.is_enabled() {
                    #[cfg(fbcode_build)]
                    {
                        // mark the command is triggered as a Rust command
                        telemetry_sample.set_rust_command(true);
                    }
                    rust_main(cmd)
                } else {
                    match fallback(None) {
                        // If the Python version of edenfsctl exited with a
                        // parse error, we should see if the Rust version
                        // exists. This helps prevent cases where rollouts
                        // are not working correctly.
                        Ok(PYTHON_EDENFSCTL_EX_USAGE) => {
                            #[cfg(fbcode_build)]
                            {
                                // We expected to use Python but we were forced
                                // to fall back to Rust. Something is wrong.
                                telemetry_sample.set_rust_fallback(true);
                                // mark the command is triggered as a Rust command
                                telemetry_sample.set_rust_command(true);
                            }
                            eprintln!(
                                "Failed to find Python implementation; falling back to Rust."
                            );
                            rust_main(cmd)
                        }
                        res => {
                            #[cfg(fbcode_build)]
                            {
                                telemetry_sample.set_rust_fallback(false);
                                // mark the command is triggered as a Python command
                                telemetry_sample.set_rust_command(false);
                            }
                            res
                        }
                    }
                }
            }
            // If the command is defined in Rust, then --help will cause
            // try_parse() to return a DisplayHelp error.  In that case, we
            // should check whether the Rust version of the command is "enabled"
            // to decide whether to print Rust or Python help.
            //
            // If the command isn't defined in Rust then try_parse will fail
            // UnknownArgument (whether or not --help was requested) and we
            // should fall back to Python.
            //
            // Otherwise, we have encountered a different error in rust and should
            // display the rust error. We still return the python error code 64 to differentiate from
            // edenfsctl errors(2)
            Err(e) => {
                if e.kind() == clap::ErrorKind::DisplayHelp
                    || e.kind() == clap::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                {
                    if should_use_rust_help(std::env::args(), &None, &None).unwrap_or(false) {
                        e.exit()
                    } else {
                        fallback(Some(&e))
                    }
                } else if e.kind() == clap::ErrorKind::UnknownArgument
                    || e.kind() == clap::ErrorKind::InvalidSubcommand
                {
                    // Failed to parse the command. We should try to fallback to Python.
                    fallback(Some(&e))
                } else {
                    // Rust command exists, but encountered a different parsing error. Print the error
                    e.print().ok();
                    Ok(PYTHON_EDENFSCTL_EX_USAGE)
                }
            }
        }
    }
}

fn should_use_rust_help<T>(
    args: T,
    etc_eden_dir_override: &Option<&Path>,
    experimental_commands_override: &Option<Vec<&str>>,
) -> Result<bool>
where
    T: Iterator<Item = String>,
{
    // This is gross, but clap v3 doesn't let us make --help a normal bool flag.
    // This means we can't successfully parse a command when --help is
    // requested.
    // But we know that if this function is called, the subcommand requested is
    // defined in Rust. So we can just manually parse the args by skipping any
    // options provided for 'edenfsctl' until we find the subcommand name.
    let mut subcommand_name = None;
    let mut skipping = false;
    for arg in args.skip(1) {
        if [
            "--config-dir".to_string(),
            "--etc-eden-dir".to_string(),
            "--home-dir".to_string(),
        ]
        .contains(&arg)
        {
            // handle skipping global option pair names
            skipping = true;
            continue;
        } else if skipping {
            // handle skipping global option pair values
            skipping = false;
            continue;
        } else if arg.starts_with("-") {
            // handle skipping global option flags, e.g. --version, -v, --debug
            continue;
        } else {
            subcommand_name = Some(arg);
            break;
        }
    }
    match subcommand_name {
        Some(name) => Ok(is_command_enabled_in_rust(
            &name,
            &etc_eden_dir_override.map(Path::to_owned),
            experimental_commands_override,
        )),
        None => Ok(false), // we are safe by always falling back to Python
    }
}

#[fbinit::main]
fn main(_fb: FacebookInit) -> Result<()> {
    // NOTE: if you are considering passing `FacebookInit` down, you may want to check
    // [`fbinit::expect_init`].
    #[cfg(fbcode_build)]
    let mut sample = CliUsageSample::build();

    #[cfg(not(fbcode_build))]
    let mut sample = CliUsageSample;

    let scenario = failpoint::setup_fail_points();

    fail_point!("edenfsctl:main");

    let code = match wrapper_main(&mut sample) {
        Ok(code) => Ok(code),
        Err(e) => {
            #[cfg(fbcode_build)]
            sample.set_exception(&e);
            Err(e)
        }
    };

    if let Some(scenario) = scenario {
        failpoint::teardown_fail_points(scenario);
    }

    #[cfg(fbcode_build)]
    {
        sample.set_exit_code(*code.as_ref().unwrap_or(&1));
        send(EDENFSCTL_CLI_USAGE.to_string(), sample.sample);
    }

    match code {
        Ok(code) => std::process::exit(code),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::should_use_rust_help;

    macro_rules! args {
        ( $( $x:expr ),* ) => (
            {
                let mut v = Vec::new();
                $(
                    v.push($x.to_string());
                )*
                v.into_iter()
            }
        );
    }

    #[test]
    fn test_should_use_rust_help() -> Result<()> {
        assert!(should_use_rust_help(
            args!["eden.exe", "minitop"],
            &None,
            &None
        )?);
        {
            let dir = TempDir::new()?;
            assert!(!should_use_rust_help(
                args!["eden.exe", "debug"],
                &Some(dir.path()),
                &Some(vec!["debug"])
            )?,);
            assert!(!should_use_rust_help(
                args![
                    "eden.exe",
                    "--config-dir",
                    "/home/scm/local/eden-dev-state",
                    "debug"
                ],
                &Some(dir.path()),
                &Some(vec!["debug"])
            )?,);
            assert!(!should_use_rust_help(
                args!["eden.exe", "debug"],
                &Some(dir.path()),
                &Some(vec!["debug"])
            )?,);
            assert!(should_use_rust_help(
                args!["eden.exe", "--debug", "debug"],
                &Some(dir.path()),
                &None
            )?,);
            assert!(!should_use_rust_help(
                args!["eden.exe", "--debug", "debug"],
                &Some(dir.path()),
                &Some(vec!["debug"])
            )?,);
        }
        {
            let dir = TempDir::new()?;
            let rollout_path = dir.path().join("edenfsctl_rollout.json");
            let mut rollout_file = File::create(rollout_path)?;
            writeln!(rollout_file, r#"{{"debug": true}}"#)?;

            assert!(should_use_rust_help(
                args!["eden.exe", "debug"],
                &Some(dir.path()),
                &Some(vec!["debug"])
            )?,);
        }
        Ok(())
    }
}
