/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::*;
use crate::protocol::{JsonProtocol, Protocol};
use crate::queries::*;
use failure::{bail, Fallible as Result};
use std::io::BufReader;
use std::marker::PhantomData;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub trait Transport {
    fn watch_project<P: AsRef<Path>>(&mut self, path: P) -> Result<WatchProjectResponse>;

    fn query<P: AsRef<Path>>(
        &mut self,
        params: QueryRequestParams,
        path: P,
    ) -> Result<QueryResponse>;

    fn state_enter<P: AsRef<Path>>(
        &mut self,
        params: StateEnterParams,
        path: P,
    ) -> Result<StateEnterResponse>;

    fn state_leave<P: AsRef<Path>>(
        &mut self,
        params: StateLeaveParams,
        path: P,
    ) -> Result<StateLeaveResponse>;
}

/// Implementations:

#[cfg(unix)]
pub mod unix_socket_transport {
    /// local unix domain socket transport */
    use self::command_line_transport::CommandLineTransport;
    use crate::transport::*;
    use std::env;
    use std::os::unix::net::UnixStream;

    pub struct UnixSocketTransport<SP, RP>
    where
        SP: Protocol, // send protocol
        RP: Protocol, // receive protocol
    {
        _phantom: PhantomData<(SP, RP)>,
        sockname: Option<PathBuf>,
        stream: Option<UnixStream>,
        socket_read_timeout: u64, // read timeout in sec
    }

    impl<SP, RP> UnixSocketTransport<SP, RP>
    where
        SP: Protocol, // send protocol
        RP: Protocol, // receive protocol
    {
        pub fn new() -> UnixSocketTransport<SP, RP> {
            UnixSocketTransport::<SP, RP> {
                _phantom: PhantomData,
                sockname: None,
                stream: None,
                socket_read_timeout: 30,
            }
        }

        pub fn with_read_timeout(&mut self, timeout: u64) -> &mut UnixSocketTransport<SP, RP> {
            self.socket_read_timeout = timeout;
            self
        }

        fn get_sock_name(&mut self) -> Result<&PathBuf> {
            match self.sockname {
                Some(ref s) => Ok(s),
                None => {
                    let envsockname = env::var("WATCHMAN_SOCK");
                    if let Ok(envsockname) = envsockname {
                        self.sockname = Some(PathBuf::from(envsockname));
                    } else {
                        let mut transport = CommandLineTransport::<RP>::new();
                        let resp = transport.get_sock_name()?;
                        if let Some(sockname) = resp.sockname {
                            self.sockname = Some(sockname.0);
                        } else {
                            return Err(ErrorKind::UnixSocketTransportError(
                                "get_sock_name",
                                "can't get sockname".into(),
                            )
                            .into());
                        }
                    }
                    match self.sockname {
                        Some(ref s) => Ok(s),
                        None => bail!("unable to determine sockname"),
                    }
                }
            }
        }

        fn rpc<Request, Response>(
            &mut self,
            cmd: &'static str,
            request: Request,
        ) -> Result<Response>
        where
            Request: serde::Serialize,
            for<'de> Response: serde::Deserialize<'de>,
        {
            if self.stream.is_none() {
                self.stream = Some(UnixStream::connect(self.get_sock_name()?)?);
            }
            let streamref = self.stream.as_mut().unwrap();
            streamref.set_read_timeout(Some(Duration::from_secs(self.socket_read_timeout)))?;
            SP::write::<Request, _>(streamref, &request)?;
            let resp = RP::read(&mut BufReader::new(streamref))?;
            match resp {
                RequestResult::Error(err) => {
                    Err(ErrorKind::UnixSocketTransportError(cmd, err.error).into())
                }
                RequestResult::Ok(r) => Ok(r),
            }
        }
    }

    impl<SP, RP> Transport for UnixSocketTransport<SP, RP>
    where
        SP: Protocol, // send protocol
        RP: Protocol, // receive protocol
    {
        fn watch_project<P: AsRef<Path>>(&mut self, path: P) -> Result<WatchProjectResponse> {
            let request = WatchProjectRequest(WATCH_PROJECT, path.as_ref().to_owned());
            self.rpc(WATCH_PROJECT, &request)
        }

        fn query<P: AsRef<Path>>(
            &mut self,
            params: QueryRequestParams,
            path: P,
        ) -> Result<QueryResponse> {
            let resp: QueryResponse =
                self.rpc(QUERY, QueryRequest(QUERY, path.as_ref().to_owned(), params))?;
            Ok(resp)
        }

        fn state_enter<P: AsRef<Path>>(
            &mut self,
            params: StateEnterParams,
            path: P,
        ) -> Result<StateEnterResponse> {
            let request = StateEnterRequest(STATE_ENTER, path.as_ref().to_owned(), params);
            self.rpc(STATE_ENTER, &request)
        }

        fn state_leave<P: AsRef<Path>>(
            &mut self,
            params: StateLeaveParams,
            path: P,
        ) -> Result<StateLeaveResponse> {
            let request = StateLeaveRequest(STATE_LEAVE, path.as_ref().to_owned(), params);
            self.rpc(STATE_LEAVE, &request)
        }
    }
}

#[cfg(unix)]
pub mod command_line_transport {
    use crate::transport::*;
    /// command line transport, required installed watchman client
    use std::process::{Command, Stdio};
    use timeout_readwrite::TimeoutReader;

    /// This transport only supports json protocol as send protocol
    /// Receive protocol can be customized

    pub struct CommandLineTransport<RP>
    where
        RP: Protocol, // receive protocol
    {
        _phantom: PhantomData<RP>,
        read_timeout_sec: u64,
    }

    impl<RP> CommandLineTransport<RP>
    where
        RP: Protocol, // receive protocol
    {
        pub fn new() -> CommandLineTransport<RP> {
            CommandLineTransport::<RP> {
                _phantom: PhantomData,
                read_timeout_sec: 30,
            }
        }

        pub fn with_read_timeout(&mut self, timeout: u64) -> &mut CommandLineTransport<RP> {
            self.read_timeout_sec = timeout;
            self
        }

        fn rpc<Request, Response>(
            &mut self,
            cmd: &'static str,
            request: Request,
        ) -> Result<Response>
        where
            Request: serde::Serialize,
            for<'de> Response: serde::Deserialize<'de>,
        {
            let output_encording = format!("--output-encoding={}", RP::name());

            let mut child = Command::new("watchman")
                .arg("--json-command")
                .arg("--no-pretty")
                .arg(output_encording)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            // with pipes it is safe to unwrap
            JsonProtocol::write(child.stdin.as_mut().unwrap(), &request)?;
            drop(child.stdin.take());

            let mut reader = BufReader::new(TimeoutReader::new(
                child.stdout.unwrap(),
                Duration::from_secs(self.read_timeout_sec),
            ));
            let res = RP::read::<RequestResult<Response>, _>(&mut reader)?;
            match res {
                RequestResult::Error(err) => {
                    Err(ErrorKind::CommandLineTransportError(cmd, err.error).into())
                }
                RequestResult::Ok(r) => Ok(r),
            }
        }

        pub fn get_sock_name(&mut self) -> Result<GetSockNameResponse> {
            let request = GetSockNameRequest((GET_SOCKNAME,));
            self.rpc(GET_SOCKNAME, request)
        }
    }

    impl<RP> Transport for CommandLineTransport<RP>
    where
        RP: Protocol, // receive protocol
    {
        fn watch_project<P: AsRef<Path>>(&mut self, path: P) -> Result<WatchProjectResponse> {
            let request = WatchProjectRequest(WATCH_PROJECT, path.as_ref().to_owned());
            self.rpc(WATCH_PROJECT, request)
        }

        fn query<P: AsRef<Path>>(
            &mut self,
            params: QueryRequestParams,
            path: P,
        ) -> Result<QueryResponse> {
            let resp: QueryResponse =
                self.rpc(QUERY, QueryRequest(QUERY, path.as_ref().to_owned(), params))?;
            Ok(resp)
        }

        fn state_enter<P: AsRef<Path>>(
            &mut self,
            _params: StateEnterParams,
            _path: P,
        ) -> Result<StateEnterResponse> {
            Err(ErrorKind::CommandLineTransportError(STATE_ENTER, "unsupported".into()).into())
        }

        fn state_leave<P: AsRef<Path>>(
            &mut self,
            _params: StateLeaveParams,
            _path: P,
        ) -> Result<StateLeaveResponse> {
            Err(ErrorKind::CommandLineTransportError(STATE_LEAVE, "unsupported".into()).into())
        }
    }
}

pub mod windows_named_pipe_transport {
    use crate::protocol::Protocol;
    use crate::transport::*;
    use std::marker::PhantomData;

    pub struct WindowsNamedPipeTransport<SP, RP>
    where
        SP: Protocol, // send protocol
        RP: Protocol, // receive protocol
    {
        _phantom: PhantomData<(SP, RP)>,
    }

    impl<SP, RP> WindowsNamedPipeTransport<SP, RP>
    where
        SP: Protocol,
        RP: Protocol,
    {
        pub fn new() -> WindowsNamedPipeTransport<SP, RP> {
            WindowsNamedPipeTransport::<SP, RP> {
                _phantom: PhantomData,
            }
        }
    }

    impl<SP, RP> Transport for WindowsNamedPipeTransport<SP, RP>
    where
        SP: Protocol,
        RP: Protocol,
    {
        fn watch_project<P: AsRef<Path>>(&mut self, _path: P) -> Result<WatchProjectResponse> {
            unimplemented!()
        }

        fn query<P: AsRef<Path>>(
            &mut self,
            _params: QueryRequestParams,
            _path_root: P,
        ) -> Result<QueryResponse> {
            unimplemented!()
        }

        fn state_enter<P: AsRef<Path>>(
            &mut self,
            _params: StateEnterParams,
            _path: P,
        ) -> Result<StateEnterResponse> {
            unimplemented!()
        }

        fn state_leave<P: AsRef<Path>>(
            &mut self,
            _params: StateLeaveParams,
            _path: P,
        ) -> Result<StateLeaveResponse> {
            unimplemented!()
        }
    }
}
