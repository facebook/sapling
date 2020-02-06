/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::Display;
use std::io::{self, Write};

use bytes::{Bytes, BytesMut};
use futures::{stream, Stream};
use futures_ext::StreamExt;
use itertools::Itertools;

use crate::handler::OutputStream;
use crate::{batch, Response, SingleResponse};

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
    match response {
        Response::Batch(resps) => {
            let separator = Bytes::from(&b";"[..]);
            let escaped_results = resps
                .into_iter()
                .map(move |resp| Bytes::from(batch::escape(&encode_cmd(resp))));

            let separated_results = escaped_results.intersperse(separator);
            let separated_results: Vec<_> = separated_results.collect();
            let mut len = 0;
            for res in separated_results.iter() {
                len += res.len();
            }
            let len = format!("{}\n", len);
            let len = stream::once(Ok(Bytes::from(len.as_bytes())));

            len.chain(stream::iter_ok(separated_results.into_iter()))
                .boxify()
        }
        Response::Single(resp) => encode_single(resp),
    }
}

fn encode_single(response: SingleResponse) -> OutputStream {
    let is_stream = response.is_stream();
    let res = encode_cmd(response);
    if is_stream {
        stream::once(Ok(res)).boxify()
    } else {
        stream::iter_ok(vec![
            Bytes::from(format!("{}\n", res.len()).as_bytes()),
            res,
        ])
        .boxify()
    }
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

        ClientTelemetry(hostname) => Bytes::from(hostname),

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

        Knownnodes(knowns) => {
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

        ListKeysPatterns(res) => res
            .into_iter()
            .map(|(bookmark, hash)| format!("{}\t{}", bookmark, hash))
            .intersperse(String::from("\n"))
            .collect::<String>()
            .into(),

        Branchmap(_res) => {
            // We have no plans to support mercurial branches and hence no plans for branchmap,
            // so just return fake response.
            Bytes::new()
        }

        StreamOutShallow(res) => res,

        Getpackv1(res) => res,
        Getpackv2(res) => res,

        r => panic!("Response for {:?} unimplemented", r),
    }
}
