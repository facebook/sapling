/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Stdin;

use anyhow::Error;
use anyhow::bail;
use aws_config::BehaviorVersion;
use aws_sdk_cloudwatchlogs::Client;
use aws_sdk_cloudwatchlogs::error::SdkError;
use aws_sdk_cloudwatchlogs::operation::create_log_group::CreateLogGroupError;
use aws_sdk_cloudwatchlogs::operation::create_log_stream::CreateLogStreamError;
use aws_sdk_cloudwatchlogs::types::InputLogEvent;
use clap::Parser;
use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;

#[derive(Parser, Debug)]
struct Args {
    /// The log group name
    #[clap(long)]
    log_group_name: String,

    /// The log stream name
    #[clap(long)]
    log_stream_name: String,
}

async fn setup(args: &Args, client: &Client) -> Result<(), Error> {
    match client
        .create_log_group()
        .log_group_name(args.log_group_name.clone())
        .send()
        .await
    {
        Ok(_) => {}
        Err(SdkError::ServiceError(err)) => match err.err() {
            CreateLogGroupError::ResourceAlreadyExistsException(_) => {}
            _ => {
                bail!("Got error creating log group: {}", err.err());
            }
        },
        Err(e) => {
            bail!("Got error creating log group: {}", e);
        }
    };

    match client
        .create_log_stream()
        .log_group_name(args.log_group_name.clone())
        .log_stream_name(args.log_stream_name.clone())
        .send()
        .await
    {
        Ok(_) => {}
        Err(SdkError::ServiceError(err)) => match err.err() {
            CreateLogStreamError::ResourceAlreadyExistsException(_) => {}
            _ => {
                bail!("Got error creating log stream: {}", err.err());
            }
        },
        Err(e) => {
            bail!("Got error creating log stream: {}", e);
        }
    };

    Ok(())
}

#[derive(Debug, Deserialize)]
struct ScubaEntry {
    int: Map<String, Value>,
    // we don't care about the rest of the fields
}

async fn tail(args: &Args, f: Stdin, client: &Client) -> Result<(), Error> {
    let mut processed = 0;

    loop {
        let mut line = String::new();
        let num_bytes = match f.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => match e.kind() {
                std::io::ErrorKind::Interrupted => continue,
                _ => bail!("Error reading line: {}", e),
            },
        };
        if num_bytes == 0 {
            // we are at EOF. Sleep, then try again
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        processed += 1;
        if processed % 100 == 0 {
            println!("Scuba: processed {} lines", processed);
        }

        let event = entry_to_event(line)?;

        client
            .put_log_events()
            .log_group_name(args.log_group_name.clone())
            .log_stream_name(args.log_stream_name.clone())
            .log_events(event)
            .send()
            .await?;
    }
}

fn entry_to_event(line: String) -> Result<InputLogEvent, Error> {
    let entry: ScubaEntry = serde_json::from_str(&line)?;
    let ts = match entry.int.get("time") {
        Some(v) => v.as_i64().unwrap() * 1000,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64,
    };

    let event = InputLogEvent::builder()
        .message(line)
        .timestamp(ts)
        .build()?;

    Ok(event)
}

#[::tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);

    setup(&args, &client).await?;

    tail(&args, std::io::stdin(), &client).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use assert_approx_eq::assert_approx_eq;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_entry_to_event() -> Result<(), Error> {
        let line = r#"{"int":{"time":1672530000,"weight":1},"normal":{"action":"start","command":"edenfsctl"}}"#;
        let result = entry_to_event(line.to_string())?;
        assert_eq!(result.timestamp(), 1672530000000);
        assert_eq!(result.message(), line);

        // the time field should always be present, but just in case
        let line = r#"{"int":{"weight":1},"normal":{"action":"start","command":"edenfsctl"}}"#;
        let result = entry_to_event(line.to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64;
        assert_approx_eq!(result.timestamp(), now, 1000);
        assert_eq!(result.message(), line);

        Ok(())
    }
}
