/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::time::Duration;
use std::vec;

use configmodel::Config;
use configmodel::ConfigExt;
use url::Host;
use url::Url;

#[derive(Debug)]
pub enum HostError {
    Config(String),
    DNS(io::Error),
    TCP(io::Error),
}

#[derive(Debug)]
pub enum Diagnosis {}

pub struct Doctor {
    // Allow tests to stub DNS responses.
    dns_lookup: Box<dyn Fn(&str) -> io::Result<vec::IntoIter<SocketAddr>>>,
    tcp_connect_timeout: Duration,
}

fn real_dns_lookup(host_port: &str) -> io::Result<vec::IntoIter<SocketAddr>> {
    host_port.to_socket_addrs()
}

impl Doctor {
    pub fn new() -> Self {
        Doctor {
            dns_lookup: Box::new(real_dns_lookup),
            tcp_connect_timeout: Duration::from_secs(1),
        }
    }

    fn check_host(&self, url: &Url) -> Result<(), HostError> {
        let port = url.port().unwrap_or(443);

        let sock_addrs: Vec<SocketAddr> = match url.host() {
            None => {
                return Err(HostError::Config(format!("url {} has no host", url)));
            }
            Some(Host::Domain(host)) => match (self.dns_lookup)(&format!("{}:{}", host, port)) {
                Err(err) => return Err(HostError::DNS(err)),
                Ok(addrs) => addrs.collect(),
            },
            Some(Host::Ipv4(ip)) => vec![SocketAddr::new(ip.into(), port)],
            Some(Host::Ipv6(ip)) => vec![SocketAddr::new(ip.into(), port)],
        };

        if sock_addrs.is_empty() {
            return Err(HostError::DNS(io::Error::new(
                io::ErrorKind::Other,
                format!("{:?} resolved to 0 IPs", url.host()),
            )));
        }

        for i in 0..sock_addrs.len() {
            match TcpStream::connect_timeout(&sock_addrs[i], self.tcp_connect_timeout) {
                Err(err) => {
                    if i == sock_addrs.len() - 1 {
                        return Err(HostError::TCP(err));
                    }
                }
                Ok(_) => break,
            }
        }

        Ok(())
    }

    pub fn diagnose(&self, _config: &dyn Config) -> Result<(), Diagnosis> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn test_check_host() {
        let mut doc = Doctor::new();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // No DNS lookup required - can connect directly to IP.
        {
            doc.dns_lookup = Box::new(move |_host_port| -> io::Result<vec::IntoIter<SocketAddr>> {
                panic!("dns lookup!")
            });


            let url = Url::parse(&format!("https://{}:{}", addr.ip(), addr.port())).unwrap();
            assert!(doc.check_host(&url).is_ok());
        }

        // DNS lookup is okay.
        {
            doc.dns_lookup = Box::new(move |_host_port| -> io::Result<vec::IntoIter<SocketAddr>> {
                Ok(vec![addr].into_iter())
            });

            let url = Url::parse(&format!("https://localhost:{}", addr.port())).unwrap();
            assert!(doc.check_host(&url).is_ok());
        }

        // DNS lookup fails.
        {
            doc.dns_lookup = Box::new(move |_host_port| -> io::Result<vec::IntoIter<SocketAddr>> {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "some sort of DNS failure!",
                ))
            });

            let url = Url::parse(&format!("https://localhost:{}", addr.port())).unwrap();
            assert!(matches!(doc.check_host(&url), Err(HostError::DNS(_))));
        }

        // No one is listening.
        {
            doc.tcp_connect_timeout = Duration::from_millis(1);

            let url = Url::parse("https://169.254.0.1:1234").unwrap();
            assert!(matches!(doc.check_host(&url), Err(HostError::TCP(_))))
        }
    }
}
