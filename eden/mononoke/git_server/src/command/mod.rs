/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use gix_packetline::PacketLineRef;
use gix_packetline::StreamingPeekableIter;
use gix_transport::bstr::BString;
use gix_transport::bstr::ByteSlice;

pub use self::fetch::FetchArgs;
pub use self::ls_refs::LsRefsArgs;

mod fetch;
mod ls_refs;

const COMMAND_PREFIX: &[u8] = b"command=";
const LS_REFS_COMMAND: &[u8] = b"ls-refs";
const FETCH_COMMAND: &[u8] = b"fetch";

#[derive(Debug, Clone)]
pub enum Command {
    LsRefs(LsRefsArgs),
    Fetch(FetchArgs),
}

#[derive(Debug, Clone)]
pub struct RequestCommand {
    pub command: Command,
    pub capability_list: Vec<BString>,
}

impl RequestCommand {
    pub fn parse_from_packetline(args: &[u8]) -> anyhow::Result<Self> {
        let mut command_token = vec![];
        let mut capability_list = vec![];
        let mut tokens = StreamingPeekableIter::new(args, &[PacketLineRef::Delimiter], true);
        while let Some(token) = tokens.read_line() {
            let token = token.context("Failed to read line from packetline")??;
            if let PacketLineRef::Data(data) = token {
                if let Some(command_type) = data.strip_prefix(COMMAND_PREFIX) {
                    command_token = command_type.trim().to_vec();
                } else {
                    capability_list.push(BString::new(data.trim().to_vec()));
                }
            } else {
                anyhow::bail!("Unexpected token {:?} in packetline", token);
            }
        }
        let remaining = tokens.into_inner();
        let command = match command_token.as_slice() {
            LS_REFS_COMMAND => Command::LsRefs(LsRefsArgs::parse_from_packetline(remaining)?),
            FETCH_COMMAND => Command::Fetch(FetchArgs::parse_from_packetline(remaining)?),
            unknown_command => {
                anyhow::bail!("Unknown git protocol V2 command {:?}", unknown_command)
            }
        };
        Ok(RequestCommand {
            command,
            capability_list,
        })
    }
}
