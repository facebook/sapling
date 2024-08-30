/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::prelude::*;
use std::io::BufReader;
use std::io::BufWriter;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use futures::future;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct RetryableCommand<'a> {
    command: Vec<&'a str>,
    retries: usize,
}

async fn write_stream<W>(writer: Arc<Mutex<BufWriter<W>>>, input: Arc<Vec<u8>>) -> Result<()>
where
    W: std::io::Write + std::marker::Send + std::marker::Sync + 'static,
{
    tokio::spawn(async move {
        let mut writer = writer.lock().expect("poisoned lock");
        writer.write_all(&input)?;
        Ok(())
    })
    .await?
}

async fn read_stream<R>(reader: Arc<Mutex<BufReader<R>>>) -> Result<String>
where
    R: std::io::Read + std::marker::Send + std::marker::Sync + 'static,
{
    tokio::spawn(async move {
        let mut reader = reader.lock().expect("poisoned lock");
        let mut contents = String::new();
        let _ = reader.read_to_string(&mut contents);
        Ok(contents)
    })
    .await?
}

impl<'a> RetryableCommand<'a> {
    pub fn new(command: Vec<&'a str>, retries: usize) -> Self {
        RetryableCommand { command, retries }
    }

    async fn spawn_once(&self, input: Option<&[u8]>) -> Result<String> {
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

        let stdin = Arc::new(Mutex::new(BufWriter::new(
            child
                .stdin
                .take()
                .ok_or(Error::msg("unexpected missing child stdin stream"))?,
        )));

        let stdout = Arc::new(Mutex::new(BufReader::new(
            child
                .stdout
                .take()
                .ok_or(Error::msg("unexpected missing child stdout stream"))?,
        )));

        let stderr = Arc::new(Mutex::new(BufReader::new(
            child
                .stderr
                .take()
                .ok_or(Error::msg("unexpected missing child stderr stream"))?,
        )));

        let input_fut = async move {
            if let Some(input) = input {
                write_stream(stdin, Arc::new(input.to_vec())).await
            } else {
                Ok(())
            }
        };

        let output_fut = async move { read_stream(stdout).await };
        let error_fut = async move { read_stream(stderr).await };

        let (_, output, error) = future::try_join3(input_fut, output_fut, error_fut).await?;

        let status = child.wait()?;

        if !status.success() {
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

    pub async fn spawn(&self, input: Option<&[u8]>) -> Result<String> {
        let mut attempt = 0;
        loop {
            match self.spawn_once(input).await {
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

    pub async fn spawn_commands(&self) -> Result<String> {
        let first_command = self
            .commands
            .first()
            .ok_or(Error::msg("No commands to run!"))?;
        let mut prev_output = first_command.spawn(None).await?;
        for command in self.commands.iter().skip(1) {
            prev_output = command.spawn(Some(prev_output.as_bytes())).await?;
        }
        Ok(prev_output)
    }
}
