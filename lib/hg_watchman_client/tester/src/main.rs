// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Test binary for manual testing

use clap::{arg_enum, value_t, App, Arg};
use watchman_client::protocol::{BserProtocol, JsonProtocol};
use watchman_client::transport::command_line_transport::CommandLineTransport;
use watchman_client::transport::unix_socket_transport::UnixSocketTransport;
use watchman_client::transport::Transport;

arg_enum! {
    #[allow(non_camel_case_types)]
    #[derive(Debug)]
    enum Transports {
        command_line_transport,
        unix_socket_transport,
        windows_named_pipe_transport
    }
}

arg_enum! {
    #[allow(non_camel_case_types)]
    #[derive(Debug)]
    enum Protocols {
        bser,
        json
    }
}

arg_enum! {
    #[allow(non_camel_case_types)]
    #[derive(Debug)]
    enum Tests {
        test_query_dirs,
        test_query_files,
        test_state_filemerge
    }
}

macro_rules! println_result {
    ($x:expr) => {
        match $x {
            Err(e) => eprintln!("{}", e),
            Ok(r) => println!(
                "{}",
                JsonProtocol::to_string_pretty(&r).expect("json parsed")
            ),
        }
    };
}

mod test_client {
    use failure::bail;
    use hg_watchman_client::{
        HgWatchmanClient, QueryResponse, StateEnterResponse, StateLeaveResponse,
    };
    use serde::{Deserialize, Serialize};
    use std::fs::OpenOptions;
    use std::io;
    use std::path::PathBuf;
    use std::process::Command;
    use std::str::from_utf8;
    use std::time::Instant;
    use watchman_client::protocol::{JsonProtocol, Protocol};
    use watchman_client::transport::Transport;

    type Result<T> = std::result::Result<T, failure::Error>;

    static FILENAME: &str = "tester_watchmanstate";

    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct ClockState {
        pub query_dirs_last_clock: Option<String>,
        pub query_files_last_clock: Option<String>,
    }

    pub struct TestClient<T>
    where
        T: Transport,
    {
        pub client: HgWatchmanClient<T>,
    }

    impl<T> TestClient<T>
    where
        T: Transport,
    {
        pub fn new(transport: T) -> TestClient<T> {
            TestClient {
                client: HgWatchmanClient::new(transport, {
                    let output = Command::new("hg").arg("root").output().unwrap();
                    PathBuf::from((from_utf8(&output.stdout).unwrap()).trim().to_string())
                }),
            }
        }

        fn get_watchman_state_filename(&self) -> Result<PathBuf> {
            let mut filename = self.client.repo_path.clone();
            filename.push(".hg");
            filename.push(FILENAME);
            Ok(filename)
        }

        fn read_watchman_state(&self) -> Result<ClockState> {
            let filename = self.get_watchman_state_filename()?;
            match OpenOptions::new().read(true).open(filename) {
                Ok(ref mut file) => JsonProtocol::read(&mut io::BufReader::new(file)),
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(ClockState::default()),
                Err(err) => bail!(err),
            }
        }

        fn write_watchman_state(&mut self, state: &ClockState) -> Result<()> {
            let filename = self.get_watchman_state_filename()?;
            let mut watchmanstate = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(filename)?;
            JsonProtocol::write(&mut watchmanstate, &state)?;
            Ok(())
        }

        // TEST Requests

        pub fn query_files(&mut self) -> Result<QueryResponse> {
            println!("hint: returns list of modified files since the previous run");
            let now = Instant::now();
            self.client.watch_project()?;
            let mut clock_state = self.read_watchman_state()?;
            let res = self
                .client
                .query_files(None, None, clock_state.query_files_last_clock);
            match res {
                Ok(ref r) => {
                    clock_state.query_files_last_clock = r.clock.clone();
                    self.write_watchman_state(&clock_state)?;
                }
                Err(_) => {}
            }
            let end = now.elapsed();
            println!(
                "[query_files] {} sec {} ms",
                end.as_secs(),
                end.subsec_nanos() as u64 / 1_000_000
            );
            res
        }

        pub fn query_dirs(&mut self) -> Result<QueryResponse> {
            println!("hint: returns list of modified directories since the previous run");
            let now = Instant::now();
            self.client.watch_project()?;
            let mut clock_state = self.read_watchman_state()?;
            let res = self
                .client
                .query_dirs(None, None, clock_state.query_dirs_last_clock);
            match res {
                Ok(ref r) => {
                    clock_state.query_dirs_last_clock = r.clock.clone();
                    self.write_watchman_state(&clock_state)?;
                }
                Err(_) => {}
            }
            let end = now.elapsed();
            println!(
                "[query_dirs] {} sec {} ms",
                end.as_secs(),
                end.subsec_nanos() as u64 / 1_000_000
            );
            res
        }

        pub fn state_filemerge_enter(&mut self) -> Result<StateEnterResponse> {
            let now = Instant::now();
            let path = PathBuf::from("fbcode");
            let res = self.client.state_filemerge_enter(&path);
            let end = now.elapsed();
            println!(
                "[state_filemerge_enter] {} sec {} ms",
                end.as_secs(),
                end.subsec_nanos() as u64 / 1_000_000
            );
            res
        }

        pub fn state_filemerge_leave(&mut self) -> Result<StateLeaveResponse> {
            let now = Instant::now();
            let path = PathBuf::from("fbcode");
            let res = self.client.state_filemerge_leave(&path);
            let end = now.elapsed();
            println!(
                "[state_filemerge_leave] {} sec {} ms",
                end.as_secs(),
                end.subsec_nanos() as u64 / 1_000_000
            );
            res
        }
    }
}

fn main() {
    let matches = App::new("tester app")
        .version("1.0.0")
        .args(&[
            Arg::from_usage("-t, --transport [transport]")
                .possible_values(&Transports::variants())
                .required(true)
                .default_value("unix_socket_transport"),
            Arg::from_usage("-p, --protocol [protocol]")
                .possible_values(&Protocols::variants())
                .required(true)
                .default_value("json"),
            Arg::from_usage("-r, --run [run test]")
                .required(true)
                .possible_values(&Tests::variants()),
        ])
        .get_matches();

    let transport = value_t!(matches.value_of("transport"), Transports).unwrap();
    let protocol = value_t!(matches.value_of("protocol"), Protocols).unwrap();
    let test = value_t!(matches.value_of("run"), Tests).unwrap();

    match transport {
        Transports::command_line_transport => match protocol {
            Protocols::bser => run(CommandLineTransport::<BserProtocol>::new(), test),
            Protocols::json => run(CommandLineTransport::<JsonProtocol>::new(), test),
        },
        Transports::unix_socket_transport => match protocol {
            Protocols::bser => run(
                UnixSocketTransport::<BserProtocol, BserProtocol>::new(),
                test,
            ),
            Protocols::json => run(
                UnixSocketTransport::<JsonProtocol, JsonProtocol>::new(),
                test,
            ),
        },
        Transports::windows_named_pipe_transport => unimplemented!(),
    };
}

fn run<T>(transport: T, test: Tests)
where
    T: Transport,
{
    let mut client = test_client::TestClient::new(transport);

    match test {
        Tests::test_query_files => test_query_files(&mut client),
        Tests::test_query_dirs => test_query_dirs(&mut client),
        Tests::test_state_filemerge => test_state_filemerge(&mut client),
    }
}

fn test_query_files<T>(client: &mut test_client::TestClient<T>)
where
    T: Transport,
{
    println!("test: running command 'query_files'");
    println_result!(client.query_files());
}

fn test_query_dirs<T>(client: &mut test_client::TestClient<T>)
where
    T: Transport,
{
    println!("test: running command 'query_dirs'");
    println_result!(client.query_dirs());
}

fn test_state_filemerge<T>(client: &mut test_client::TestClient<T>)
where
    T: Transport,
{
    println!("test: running command 'state_filemerge_enter'");
    println_result!(client.state_filemerge_enter());
    println!("test: running command 'state_filemerge_leave'");
    println_result!(client.state_filemerge_leave());
}
