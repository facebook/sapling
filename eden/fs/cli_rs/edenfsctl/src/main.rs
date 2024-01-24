/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use edenfs_commands::is_command_enabled;
#[cfg(fbcode_build)]
use edenfs_telemetry::cli_usage::CliUsageSample;
#[cfg(fbcode_build)]
use edenfs_telemetry::send;
#[cfg(fbcode_build)]
use edenfs_telemetry::EDENFSCTL_CLI_USAGE;
#[cfg(windows)]
use edenfs_utils::execute_par;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use fbinit::FacebookInit;
use tracing_subscriber::filter::EnvFilter;

fn python_fallback() -> Result<Command> {
    if let Ok(args) = std::env::var("EDENFSCTL_REAL") {
        // We might get a command starting with python.exe here instead of a simple path.
        let mut parts = args.split_ascii_whitespace();
        let binary = parts
            .next()
            .ok_or_else(|| anyhow!("invalid fallback environment variable: {:?}", args))?;
        let mut cmd = Command::new(binary);
        cmd.args(parts);
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

fn fallback(reason: Option<clap::Error>) -> Result<i32> {
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
    Ok(cmd.run()?)
}

/// This function takes care of the fallback logic, hijack supported subcommand
/// to Rust implementation and forward the rest to Python.
fn wrapper_main() -> Result<i32> {
    if std::env::var("EDENFSCTL_ONLY_RUST").is_ok() {
        let cmd = edenfs_commands::MainCommand::parse();
        rust_main(cmd)
    } else if std::env::var("EDENFSCTL_SKIP_RUST").is_ok() {
        fallback(None)
    } else {
        match edenfs_commands::MainCommand::try_parse() {
            // The command is defined in Rust, but check whether it's "enabled"
            // for Rust or else fall back to Python.
            Ok(cmd) => {
                if cmd.is_enabled() {
                    rust_main(cmd)
                } else {
                    fallback(None)
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
            Err(e) => {
                if (e.kind() == clap::ErrorKind::DisplayHelp
                    || e.kind() == clap::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand)
                    && should_use_rust_help(std::env::args(), &None).unwrap_or(false)
                {
                    e.exit()
                } else {
                    fallback(Some(e))
                }
            }
        }
    }
}

fn should_use_rust_help<T>(args: T, etc_eden_dir_override: &Option<&Path>) -> Result<bool>
where
    T: Iterator<Item = String>,
{
    // This is gross, but clap v3 doesn't let us make --help a normal bool flag.
    // This means we can't successfully parse a command when --help is
    // requested, so here we manually extract the subcommand name in order to
    // check whether it's enabled for Rust.
    let subcommand_name = args
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .ok_or(anyhow!("missing subcommand"))?;

    Ok(is_command_enabled(
        &subcommand_name,
        &etc_eden_dir_override.map(Path::to_owned),
    ))
}

#[fbinit::main]
fn main(_fb: FacebookInit) -> Result<()> {
    // NOTE: if you are considering passing `FacebookInit` down, you may want to check
    // [`fbinit::expect_init`].
    #[cfg(fbcode_build)]
    let mut sample = CliUsageSample::build();

    let code = match wrapper_main() {
        Ok(code) => Ok(code),
        Err(e) => {
            #[cfg(fbcode_build)]
            sample.set_exception(&e);
            Err(e)
        }
    };

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
        assert!(should_use_rust_help(args!["eden.exe", "minitop"], &None)?);
        {
            let dir = TempDir::new()?;
            assert!(!should_use_rust_help(
                args!["eden.exe", "redirect"],
                &Some(dir.path())
            )?,);
            assert!(!should_use_rust_help(
                args!["eden.exe", "--xyz", "redirect"],
                &Some(dir.path())
            )?,);
        }
        {
            let dir = TempDir::new()?;
            let rollout_path = dir.path().join("edenfsctl_rollout.json");
            let mut rollout_file = File::create(rollout_path)?;
            writeln!(rollout_file, r#"{{"redirect": true}}"#)?;

            assert!(should_use_rust_help(
                args!["eden.exe", "redirect"],
                &Some(dir.path())
            )?,);
        }

        Ok(())
    }
}
