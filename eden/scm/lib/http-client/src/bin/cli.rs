/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::io::prelude::*;
use std::io::stdin;
use std::io::stdout;

use anyhow::Result;
use futures::prelude::*;
use http_client::AsyncResponse;
use http_client::HttpClient;
use http_client::Request;
use structopt::StructOpt;
use url::Url;

const CERT_ENV_VAR: &str = "CERT";
const KEY_ENV_VAR: &str = "KEY";
const CA_ENV_VAR: &str = "CA";

#[derive(Debug, StructOpt)]
#[structopt(name = "http_cli", about = "Send HTTP requests")]
enum Method {
    #[structopt(about = "Send a GET request")]
    Get(Args),
    #[structopt(about = "Send a HEAD request")]
    Head(Args),
    #[structopt(about = "Send a POST request")]
    Post(Args),
    #[structopt(about = "Send a PUT request")]
    Put(Args),
}

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(help = "Request URL")]
    url: String,
    #[structopt(
        long,
        short = "H",
        help = "Headers as a series of \"Header-Name: Value\" strings"
    )]
    headers: Vec<String>,
}

impl Args {
    fn url(&self) -> Result<Url> {
        Ok(Url::parse(&self.url)?)
    }
}

/// Note that we technically don't need to use async here;
/// `Request::send()` would have sufficed. However, the
/// purpose of this binary is primarily for testing and
/// debugging the library itself, so by using async we
/// can maximize the surface area exercised.
#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    match Method::from_args() {
        Method::Get(args) => cmd_get(args).await,
        Method::Head(args) => cmd_head(args).await,
        Method::Post(args) => cmd_post(args).await,
        Method::Put(args) => cmd_put(args).await,
    }
}

async fn cmd_get(args: Args) -> Result<()> {
    let req = HttpClient::new().get(args.url()?);
    let req = add_headers(req, &args.headers);
    let req = configure_tls(req);

    let res = req.send_async().await?;
    write_response(res).await
}

async fn cmd_head(args: Args) -> Result<()> {
    let req = HttpClient::new().head(args.url()?);
    let req = add_headers(req, &args.headers);
    let req = configure_tls(req);

    let res = req.send_async().await?;
    write_response(res).await
}

async fn cmd_post(args: Args) -> Result<()> {
    eprintln!("Reading payload from stdin");
    let body = read_input()?;

    let req = HttpClient::new().post(args.url()?).body(body);
    let req = add_headers(req, &args.headers);
    let req = configure_tls(req);

    let res = req.send_async().await?;
    write_response(res).await
}

async fn cmd_put(args: Args) -> Result<()> {
    eprintln!("Reading payload from stdin");
    let body = read_input()?;

    let req = HttpClient::new().put(args.url()?).body(body);
    let req = add_headers(req, &args.headers);
    let req = configure_tls(req);

    let res = req.send_async().await?;
    write_response(res).await
}

fn read_input() -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

async fn write_response(res: AsyncResponse) -> Result<()> {
    eprintln!("Status: {:?} {}", res.version(), res.status());
    eprintln!("{:?}", res.headers());

    let body = res.into_body().decoded().try_concat().await?;

    if atty::is(atty::Stream::Stdout) {
        println!("{}", String::from_utf8_lossy(&body).escape_default())
    } else {
        stdout().write_all(&body)?;
    };

    Ok(())
}

fn configure_tls(mut req: Request) -> Request {
    if let Ok(cert) = env::var(CERT_ENV_VAR) {
        req = req.cert(cert);
    }
    if let Ok(key) = env::var(KEY_ENV_VAR) {
        req = req.key(key);
    }
    if let Ok(ca) = env::var(CA_ENV_VAR) {
        req = req.cainfo(ca);
    }
    req
}

fn add_headers(mut req: Request, headers: &[String]) -> Request {
    for header in headers {
        let (name, value) = split_header(header);
        req = req.header(name, value);
    }
    req
}

fn split_header(header: &str) -> (&str, &str) {
    let parts = header.splitn(2, ':').collect::<Vec<_>>();
    if parts.len() > 1 {
        (parts[0], parts[1].trim_start())
    } else {
        (parts[0], "")
    }
}
