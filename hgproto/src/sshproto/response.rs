// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::Display;
use std::io::{self, Write};

use bytes::{BufMut, Bytes, BytesMut};
use futures::stream;
use futures_ext::StreamExt;

use {batch, Response, SingleResponse};
use handler::OutputStream;

fn separated<I, W>(write: &mut W, iter: I, sep: &str) -> io::Result<()>
where
    I: IntoIterator,
    I::Item: Display,
    W: Write,
{
    let iter = iter.into_iter();

    let mut first = true;
    for it in iter {
        if first {
            first = false;
        } else {
            write!(write, "{}", sep)?;
        }
        write!(write, "{}", it)?;
    }
    write!(write, "\n")?;
    Ok(())
}

pub fn encode(response: Response) -> OutputStream {
    let mut out = BytesMut::new();
    match response {
        Response::Batch(resps) => {
            let escaped_results: Vec<_> = resps
                .into_iter()
                .map(|resp| batch::escape(&encode_cmd(resp)))
                .collect();
            let escaped_results = Bytes::from(escaped_results.join(&b';'));
            out.reserve(10 + escaped_results.len());
            out.put_slice(format!("{}\n", escaped_results.len()).as_bytes());
            out.put(escaped_results)
        }
        Response::Single(resp) => encode_single(resp, &mut out),
    }
    stream::once(Ok(out.freeze())).boxify()
}

fn encode_single(response: SingleResponse, out: &mut BytesMut) {
    let is_stream = response.is_stream();
    let res = encode_cmd(response);
    out.reserve(10 + res.len());
    if !is_stream {
        out.put_slice(format!("{}\n", res.len()).as_bytes());
    }
    out.put(res);
}

/// Encode the result of an individual command completion. This is used by both
/// single and batch responses encoding
fn encode_cmd(response: SingleResponse) -> Bytes {
    use SingleResponse::*;

    match response {
        Hello(map) => {
            let mut out = Vec::new();

            for (k, caps) in map {
                write!(out, "{}: {}\n", k, caps.join(" ")).expect("write to vec failed");
            }

            Bytes::from(out)
        }

        Between(vecs) => {
            let mut out = Vec::new();

            for v in vecs {
                separated(&mut out, v, " ").expect("write to vec failed");
            }

            Bytes::from(out)
        }

        Debugwireargs(res) => res,

        Heads(set) => {
            let mut out = Vec::new();

            separated(&mut out, set, " ").expect("write to vec failed");

            Bytes::from(out)
        }

        Known(knowns) => {
            let out: Vec<_> = knowns
                .into_iter()
                .map(|known| if known { b'1' } else { b'0' })
                .collect();

            Bytes::from(out)
        }

        ReadyForStream => Bytes::from(b"0\n".as_ref()),

        // TODO(luk, T25574469) The response for Unbundle should be chunked stream of bundle2
        Unbundle(res) => res,

        Getbundle(res) => res,

        Gettreepack(res) => res,

        Getfiles(res) => res,

        Lookup(res) => res,

        Listkeys(res) => {
            let mut bytes = BytesMut::new();
            for (name, key) in res {
                bytes.extend_from_slice(&name);
                bytes.extend_from_slice("\t".as_bytes());
                bytes.extend_from_slice(key.as_ref());
                bytes.extend_from_slice(&"\n".as_bytes());
            }
            bytes.freeze()
        }

        Branchmap(_res) => {
            // We have no plans to support mercurial branches and hence no plans for branchmap,
            // so just return fake response.
            Bytes::new()
        }

        StreamOutShallow(res) => res,

        r => panic!("Response for {:?} unimplemented", r),
    }
}
