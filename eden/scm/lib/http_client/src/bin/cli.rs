/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::io::{prelude::*, stdin, stdout};

use anyhow::Result;
use structopt::StructOpt;
use url::Url;

use http_client::{Request, Response};

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

fn main() -> Result<()> {
    env_logger::init();
    match Method::from_args() {
        Method::Get(args) => cmd_get(args),
        Method::Head(args) => cmd_head(args),
        Method::Post(args) => cmd_post(args),
        Method::Put(args) => cmd_put(args),
    }
}

fn cmd_get(args: Args) -> Result<()> {
    let url = args.url()?;

    let req = Request::get(&url);
    let req = add_headers(req, &args.headers);

    let creds = get_creds();
    let ca = get_ca();
    let req = configure_tls(req, &creds, &ca)?;

    write_response(req.send()?)
}

fn cmd_head(args: Args) -> Result<()> {
    let url = args.url()?;

    let req = Request::head(&url);
    let req = add_headers(req, &args.headers);

    let creds = get_creds();
    let ca = get_ca();
    let req = configure_tls(req, &creds, &ca)?;

    write_response(req.send()?)
}

fn cmd_post(args: Args) -> Result<()> {
    let url = args.url()?;

    eprintln!("Reading payload from stdin");
    let body = read_input()?;

    let req = Request::post(&url).body(body);
    let req = add_headers(req, &args.headers);

    let creds = get_creds();
    let ca = get_ca();
    let req = configure_tls(req, &creds, &ca)?;

    write_response(req.send()?)
}

fn cmd_put(args: Args) -> Result<()> {
    let url = args.url()?;

    eprintln!("Reading payload from stdin");
    let body = read_input()?;

    let req = Request::put(&url).body(body);
    let req = add_headers(req, &args.headers);

    let creds = get_creds();
    let ca = get_ca();
    let req = configure_tls(req, &creds, &ca)?;

    write_response(req.send()?)
}

fn read_input() -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

fn write_response(res: Response) -> Result<()> {
    eprintln!("Status: {}", res.status);
    eprintln!("{:?}", &res.headers);

    if atty::is(atty::Stream::Stdout) {
        println!("{}", String::from_utf8_lossy(&res.body).escape_default())
    } else {
        stdout().write_all(&res.body)?;
    };

    Ok(())
}

fn configure_tls<'a>(
    mut req: Request<'a>,
    creds: &'a Option<(String, String)>,
    ca: &'a Option<String>,
) -> Result<Request<'a>> {
    if let Some((cert, key)) = creds {
        req = req.creds(cert, key)?;
    }
    if let Some(ca) = ca {
        req = req.cainfo(ca)?;
    }
    Ok(req)
}

fn get_creds() -> Option<(String, String)> {
    let cert = env::var(CERT_ENV_VAR).ok()?;
    let key = env::var(KEY_ENV_VAR).ok()?;
    Some((cert, key))
}

fn get_ca() -> Option<String> {
    env::var(CA_ENV_VAR).ok()
}

fn add_headers<'a>(mut req: Request<'a>, headers: &'a [String]) -> Request<'a> {
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
