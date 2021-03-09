/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl top

use async_trait::async_trait;
use std::time::Duration;
use structopt::StructOpt;

use termwiz::{
    caps::Capabilities,
    input::{InputEvent, KeyCode, KeyEvent},
    terminal::buffered::BufferedTerminal,
    terminal::{new_terminal, Terminal},
    Error,
};

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Monitor EdenFS accesses by process.")]
pub struct TopCmd {
    /// Don't accumulate data; refresh the screen every update
    /// cycle.
    #[structopt(short, long)]
    ephemeral: bool,

    /// Specify the rate (in seconds) at which eden top updates.
    #[structopt(short, long, default_value = "1")]
    refresh_rate: u64,
}

impl TopCmd {
    fn main_loop(&self) -> Result<(), Error> {
        let caps = Capabilities::new_from_env()?;
        let terminal = new_terminal(caps)?;
        let mut buf = BufferedTerminal::new(terminal)?;

        buf.add_change("eden top\r\n");
        buf.flush()?;

        buf.terminal().set_raw_mode()?;
        loop {
            match buf
                .terminal()
                .poll_input(Some(Duration::from_secs(self.refresh_rate)))
            {
                Ok(Some(input)) => match input {
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('q'),
                        ..
                    }) => {
                        break;
                    }
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char(c),
                        ..
                    }) => {
                        // TODO: delete, this is just used for testing
                        buf.add_change(format!("{}\r\n", c));
                        buf.flush()?;
                    }
                    _ => {}
                },
                Ok(None) => {}
                Err(e) => {
                    print!("{:?}\r\n", e);
                    break;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl crate::Subcommand for TopCmd {
    async fn run(&self, _instance: EdenFsInstance) -> Result<ExitCode> {
        match self.main_loop() {
            Ok(_) => Ok(0),
            Err(cause) => {
                println!("Error: {}", cause);
                Ok(1)
            }
        }
    }
}
