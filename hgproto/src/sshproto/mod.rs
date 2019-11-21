/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! SSH/stdio line-oriented protocol
//!
//! References are https://www.mercurial-scm.org/wiki/SshCommandProtocol and
//! https://www.mercurial-scm.org/wiki/WireProtocol though they're scant on detail.
//!
//! The encoding is:
//! ```text
//! command := <command> '\n' <key-value>{N}
//! key-value := star | kv
//! star := '*' ' ' <count> kv{count}
//! kv := <name> ' ' <numbytes> '\n' <byte>{numbytes}
//! ```
//!
//! Where `N` is the specific number of `<key-value>` pairs expected by the command.
//!
//! The types of the values are implied by the command rather than explicitly encoded.
//!
//! pair := <hash> '-' <hash>
//! list := (<hash> ' ')* <item>
//!
//! Responses to commands are almost always:
//! ```text
//! <numbytes> '\n'
//! <byte>{numbytes}
//! ```
//!
//! The expections are requests that pass streaming arguments (f.e. unbundle). After such a
//! requests the responder should respond with
//! ```text
//! '0\n'
//! ```
//! to acknowledge readiness for processing the stream. After the stream is fully read the
//! responder should respond with a stream (no acknowledgment is required).
//!
//! Each command has its own encoding of the regular or streaming responses, although by
//! convention the streaming responses are chunked. See `hgproto/dechunker.rs` for the format of
//! chunking.

use bytes::BytesMut;
use tokio_io::codec::Decoder;

use crate::handler::{OutputStream, ResponseEncoder};
use crate::{Request, Response};

use crate::errors::*;

pub mod request;
pub mod response;

#[derive(Clone)]
pub struct HgSshCommandEncode;
#[derive(Clone)]
pub struct HgSshCommandDecode;

impl ResponseEncoder for HgSshCommandEncode {
    fn encode(&self, response: Response) -> OutputStream {
        response::encode(response)
    }
}

impl Decoder for HgSshCommandDecode {
    type Item = Request;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Request>> {
        request::parse_request(buf)
    }
}
