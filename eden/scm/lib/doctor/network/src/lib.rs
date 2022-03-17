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
pub enum Diagnosis {
    BadConfig(String),
    NoInternet(HostError),
    NoCorp(HostError),
}

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

    pub fn diagnose(&self, config: &dyn Config) -> Result<(), Diagnosis> {
        self.check_corp_connectivity(config)?;
        Ok(())
    }

    fn check_corp_connectivity(&self, config: &dyn Config) -> Result<(), Diagnosis> {
        let repo_url = config_url(config, "edenapi", "url", None)?;

        // First check for corp connectivity.
        let corp_err = match self.check_host(&repo_url) {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };

        // If we don't have corp connectivity, see if we have internet connectivity.
        let external_url = config_url(
            config,
            "doctor",
            "external-host-check-url",
            Some("https://www.facebook.com"),
        )?;

        match self.check_host(&external_url) {
            Ok(()) => Err(Diagnosis::NoCorp(corp_err)),
            Err(err) => Err(Diagnosis::NoInternet(err)),
        }
    }
}

fn config_url(
    config: &dyn Config,
    section: &str,
    field: &str,
    default: Option<&str>,
) -> Result<Url, Diagnosis> {
    let url: String = match (config.get_nonempty_opt(section, field), default) {
        (Err(err), _) => return Err(Diagnosis::BadConfig(format!("config error: {}", err))),
        (Ok(Some(url)), _) => url,
        (Ok(None), Some(default)) => default.to_string(),
        (Ok(None), None) => {
            return Err(Diagnosis::BadConfig(format!(
                "no config for {}.{}",
                section, field
            )));
        }
    };

    match Url::parse(&url) {
        Err(err) => Err(Diagnosis::BadConfig(format!(
            "invalid url {}: {}",
            url, err
        ))),
        Ok(url) => Ok(url),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
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

    #[test]
    fn test_check_diagnose() {
        let non_working_url = "https://169.254.0.1:1234";

        // Both corp and external fail.
        {
            let mut cfg = BTreeMap::new();
            cfg.insert("edenapi.url".to_string(), non_working_url.to_string());
            cfg.insert(
                "doctor.external-host-check-url".to_string(),
                non_working_url.to_string(),
            );

            let mut doc = Doctor::new();
            doc.tcp_connect_timeout = Duration::from_millis(1);

            assert!(matches!(doc.diagnose(&cfg), Err(Diagnosis::NoInternet(_))));
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_addr = listener.local_addr().unwrap();
        let working_url = format!("https://{}:{}", listener_addr.ip(), listener_addr.port());

        // External works.
        {
            let mut cfg = BTreeMap::new();
            cfg.insert("edenapi.url".to_string(), non_working_url.to_string());
            cfg.insert(
                "doctor.external-host-check-url".to_string(),
                working_url.to_string(),
            );

            let doc = Doctor::new();
            assert!(matches!(doc.diagnose(&cfg), Err(Diagnosis::NoCorp(_))));
        }

        // Corp works.
        {
            let mut cfg = BTreeMap::new();
            cfg.insert("edenapi.url".to_string(), working_url.to_string());
            cfg.insert(
                "doctor.external-host-check-url".to_string(),
                working_url.to_string(),
            );

            let doc = Doctor::new();
            assert!(matches!(doc.diagnose(&cfg), Ok(())));
        }
    }
}
