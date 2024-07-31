/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::prelude::*;
use std::process::Command;
use std::process::Stdio;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct RetryableCommand<'a> {
    command: Vec<&'a str>,
    retries: usize,
}

impl<'a> RetryableCommand<'a> {
    pub fn new(command: Vec<&'a str>, retries: usize) -> Self {
        RetryableCommand { command, retries }
    }

    fn spawn_once(&self, input: Option<&[u8]>) -> Result<String> {
        let mut command = Command::new(
            self.command
                .first()
                .ok_or(Error::msg("No commands to run!"))?,
        );

        command.args(&self.command[1..]);
        command.stderr(Stdio::piped());
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());

        let mut child = command.spawn()?;

        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .ok_or(Error::msg("unexpected missing child stdin stream"))?;
            stdin.write_all(input)?;
        }
        let mut output = String::new();
        let mut stdout = child
            .stdout
            .take()
            .ok_or(Error::msg("unexpected missing child stdout stream"))?;
        let _ = stdout.read_to_string(&mut output);
        let status = child.wait()?;
        if !status.success() {
            let mut error = String::new();
            let mut stderr = child
                .stderr
                .take()
                .ok_or(Error::msg("unexpected missing child stderr stream"))?;
            let _ = stderr.read_to_string(&mut error);
            Err(anyhow!(
                "Child process failed with exit code {} with stdout: `{}` and stderr: `{}`",
                status.code().unwrap_or(-1),
                output,
                error,
            ))
        } else {
            Ok(output)
        }
    }

    pub fn spawn(&self, input: Option<&[u8]>) -> Result<String> {
        let mut attempt = 0;
        loop {
            match self.spawn_once(input) {
                Ok(output) => return Ok(output),
                Err(e) => {
                    if attempt < self.retries {
                        attempt += 1;
                        logging::info!("Retrying command after error: {}", e);
                    } else {
                        return Err(anyhow!(
                            "Failed to execute command `{}` with error `{}`",
                            self.command.join(" "),
                            e
                        ));
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RetryablePipe<'a> {
    commands: Vec<RetryableCommand<'a>>,
}

impl<'a> RetryablePipe<'a> {
    pub fn new() -> Self {
        RetryablePipe { commands: vec![] }
    }

    fn add_command(&mut self, command: RetryableCommand<'a>) {
        self.commands.push(command);
    }

    pub fn add_retryable_command(&mut self, command: Vec<&'a str>, retries: usize) {
        let retryable_command = RetryableCommand::new(command, retries);
        self.add_command(retryable_command)
    }

    pub fn add_nonretryable_command(&mut self, command: Vec<&'a str>) {
        self.add_retryable_command(command, 0);
    }

    pub fn spawn_commands(&self) -> Result<String> {
        let first_command = self
            .commands
            .first()
            .ok_or(Error::msg("No commands to run!"))?;
        let mut prev_output = first_command.spawn(None)?;
        for command in self.commands.iter().skip(1) {
            prev_output = command.spawn(Some(prev_output.as_bytes()))?;
        }
        Ok(prev_output)
    }
}
