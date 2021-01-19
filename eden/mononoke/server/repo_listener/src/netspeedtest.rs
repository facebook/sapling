/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

const NETSPEEDTEST_MAX_NBYTES: usize = 100 * 1024 * 1024;
const NETSPEEDTEST_TIMEOUT_SECS: u64 = 10;

pub enum NetSpeedTest {
    Download(u64),
    Upload(u64),
}

pub fn parse_netspeedtest_http_params(
    headers: &HashMap<String, String>,
    method: Option<String>,
) -> Result<NetSpeedTest> {
    match method.as_deref() {
        Some("GET") => {
            if let Some(nbytes) = headers.get("x-netspeedtest-nbytes") {
                Ok(NetSpeedTest::Download(nbytes.parse::<u64>()?))
            } else {
                Err(anyhow!("missing x-netspeedtest-nbytes header"))
            }
        }
        Some("POST") => {
            if let Some(nbytes) = headers.get("content-length") {
                Ok(NetSpeedTest::Upload(nbytes.parse::<u64>()?))
            } else {
                Err(anyhow!("missing content-length header"))
            }
        }
        _ => Err(anyhow!("bad method")),
    }
}

pub async fn handle_http_netspeedtest<R, W>(rx: R, tx: W, params: NetSpeedTest) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    match params {
        NetSpeedTest::Download(nbytes) => {
            let fut = download(tx, nbytes);
            timeout(Duration::from_secs(NETSPEEDTEST_TIMEOUT_SECS), fut).await??;
        }
        NetSpeedTest::Upload(nbytes) => {
            let fut = upload(rx, tx, nbytes);
            timeout(Duration::from_secs(NETSPEEDTEST_TIMEOUT_SECS), fut).await??;
        }
    }

    Ok(())
}

async fn download<W>(mut tx: W, byte_count: u64) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut byte_count = std::cmp::min(byte_count as usize, NETSPEEDTEST_MAX_NBYTES);

    let mut header =
        create_http_header("200 Ok", vec![("Content-Length", &byte_count.to_string())]);
    header.push_str("\r\n");
    tx.write_all(header.as_bytes()).await?;

    let mut repeat = tokio::io::repeat(0x42).take(byte_count as u64);
    while byte_count > 0 {
        let bytes_read = tokio::io::copy(&mut repeat, &mut tx).await?;
        byte_count -= bytes_read as usize;
    }

    Ok(())
}

async fn upload<R, W>(rx: R, mut tx: W, byte_count: u64) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut byte_count = std::cmp::min(byte_count as usize, NETSPEEDTEST_MAX_NBYTES);
    let mut sink = tokio::io::sink();
    let mut bounded_rx = rx.take(byte_count as u64);

    while byte_count > 0 {
        let bytes_read = tokio::io::copy(&mut bounded_rx, &mut sink).await?;
        byte_count -= bytes_read as usize;
    }
    tx.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await?;

    Ok(())
}

pub fn create_http_header<'a>(status_msg: &'a str, headers: Vec<(&str, &str)>) -> String {
    let mut buf = format!("HTTP/1.1 {}\r\n", status_msg);
    for (k, v) in &headers {
        buf.push_str(k);
        buf.push_str(": ");
        buf.push_str(v);
        buf.push_str("\r\n");
    }
    buf
}
