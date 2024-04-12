/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use gix_hash::ObjectId;
use gix_packetline::PacketLineRef;
use gix_packetline::StreamingPeekableIter;
use gix_transport::bstr::ByteSlice;
use protocol::types::FetchRequest;
use protocol::types::PackfileConcurrency;

const DONE: &[u8] = b"done";
const THIN_PACK: &[u8] = b"thin-pack";
const NO_PROGRESS: &[u8] = b"no-progress";
const INCLUDE_TAG: &[u8] = b"include-tag";
const OFSET_DELTA: &[u8] = b"ofs-delta";
const WAIT_FOR_DONE: &[u8] = b"wait-for-done";
const SIDEBAND_ALL: &[u8] = b"sideband-all";
const DEEPEN_RELATIVE: &[u8] = b"deepen-relative";

const WANT_PREFIX: &[u8] = b"want ";
const HAVE_PREFIX: &[u8] = b"have ";
const PACKFILE_URIS_PREFIX: &[u8] = b"packfile-uris ";
const WANT_REF_PREFIX: &[u8] = b"want-ref ";
const FILTER_PREFIX: &[u8] = b"filter ";
const DEEPEN_NOT_PREFIX: &[u8] = b"deepen-not ";
const DEEPEN_SINCE_PREFIX: &[u8] = b"deepen-since ";
const DEEPEN_PREFIX: &[u8] = b"deepen ";
const SHALLOW_PREFIX: &[u8] = b"shallow ";

const PACKFILE_URIS_SEPARATOR: &str = ",";

/// Arguments for `fetch` command
#[derive(Clone, Debug, Default)]
pub struct FetchArgs {
    /// Indicates to the server the objects which the client wants to
    /// retrieve
    pub wants: Vec<ObjectId>,
    /// Indicates to the server the objects which the client already has
    /// locally
    pub haves: Vec<ObjectId>,
    /// Indicates to the server that negotiation should terminate (or
    /// not even begin if performing a clone) and that the server should
    /// use the information supplied in the request to construct the packfile
    pub done: bool,
    /// Request that a thin pack be sent, which is a pack with deltas
    /// which reference base objects not contained within the pack (but
    /// are known to exist at the receiving end)
    pub thin_pack: bool,
    /// Request that progress information that would normally be sent on
    /// side-band channel 2, during the packfile transfer, should not be sent
    pub no_progress: bool,
    /// Request that annotated tags should be sent if the objects they
    /// point to are being sent.
    pub include_tag: bool,
    /// Indicate that the client understands PACKv2 with delta referring
    /// to its base by position in pack rather than by an oid
    pub ofs_delta: bool,
    /// List of object Ids representing the edge of the shallow history present
    /// at the client, i.e. the set of commits that the client knows about but
    /// does not have any of their parents and their ancestors
    pub shallow: Vec<ObjectId>,
    /// Requests that the fetch/clone should be shallow having a commit
    /// depth of "deepen" relative to the server
    pub deepen: Option<u32>,
    /// Requests that the semantics of the "deepen" command be changed
    /// to indicate that the depth requested is relative to the client's
    /// current shallow boundary, instead of relative to the requested commits.
    pub deepen_relative: bool,
    /// Requests that the shallow clone/fetch should be cut at a specific time,
    /// instead of depth. The timestamp provided should be in the same format
    /// as is expected for git rev-list --max-age <timestamp>
    pub deepen_since: Option<gix_date::Time>,
    /// Requests that the shallow clone/fetch should be cut at a specific revision
    /// instead of a depth, i.e. the specified oid becomes the boundary at which the
    /// fetch or clone should stop at
    pub deepen_not: Option<ObjectId>,
    /// Request that various objects from the packfile be omitted using
    /// one of several filtering techniques
    pub filter: Option<String>,
    /// Indicates to the server that the client wants to retrieve a particular set of
    /// refs by providing the full name of the ref on the server
    pub want_refs: Vec<String>,
    /// Instruct the server to send the whole response multiplexed, not just the
    /// packfile section
    pub sideband_all: bool,
    /// Indicates to the server that the client is willing to receive URIs of any
    /// of the given protocols in place of objects in the sent packfile. Before
    /// performing the connectivity check, the client should download from all given URIs
    pub packfile_uris: Vec<String>,
    /// Indicates to the server that it should never send "ready", but should wait
    /// for the client to say "done" before sending the packfile
    pub wait_for_done: bool,
}

fn parse_oid(data: &[u8], oid_type: &[u8]) -> anyhow::Result<ObjectId> {
    ObjectId::from_hex(data).with_context(|| {
        format!(
            "Invalid {:?}object id {:?} received during fetch request",
            oid_type, data
        )
    })
}

fn bytes_to_str<'a, 'b, 'c>(
    bytes: &'a [u8],
    bytes_type: &'b str,
    arg_type: &'c str,
) -> anyhow::Result<&'a str> {
    std::str::from_utf8(bytes).with_context(|| {
        format!(
            "Invalid {} bytes {:?} received for {:?} during fetch command args parsing",
            bytes_type, arg_type, bytes
        )
    })
}

impl FetchArgs {
    /// Method determining if the fetch request is a shallow fetch request
    pub fn is_shallow(&self) -> bool {
        !self.shallow.is_empty()
            || self.deepen.is_some()
            || self.deepen_since.is_some()
            || self.deepen_not.is_some()
    }

    /// Method determining if the fetch request is a filter fetch request
    pub fn is_filter(&self) -> bool {
        self.filter.is_some()
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.deepen.is_some() && self.deepen_since.is_some() {
            anyhow::bail!(
                "deepen and deepen-since arguments cannot be provided at the same time for fetch command"
            )
        } else if self.deepen.is_some() && self.deepen_not.is_some() {
            anyhow::bail!(
                "deepen and deepen-not arguments cannot be provided at the same time for fetch command"
            )
        } else {
            Ok(())
        }
    }

    pub fn parse_from_packetline(args: &[u8]) -> anyhow::Result<Self> {
        let mut tokens = StreamingPeekableIter::new(args, &[PacketLineRef::Flush], true);
        let mut fetch_args = Self::default();
        while let Some(token) = tokens.read_line() {
            let token = token.context(
                "Failed to read line from packetline during fetch command args parsing",
            )??;
            if let PacketLineRef::Data(data) = token {
                let data = data.trim();
                if let Some(oid) = data.strip_prefix(WANT_PREFIX) {
                    fetch_args.wants.push(parse_oid(oid, WANT_PREFIX)?);
                } else if let Some(oid) = data.strip_prefix(HAVE_PREFIX) {
                    fetch_args.haves.push(parse_oid(oid, HAVE_PREFIX)?);
                } else if let Some(oid) = data.strip_prefix(SHALLOW_PREFIX) {
                    fetch_args.shallow.push(parse_oid(oid, SHALLOW_PREFIX)?);
                } else if let Some(depth) = data.strip_prefix(DEEPEN_PREFIX) {
                    let depth = bytes_to_str(depth, "depth", "deepen")?.parse::<u32>();
                    fetch_args.deepen = Some(depth.clone().with_context(|| {
                        format!(
                            "Invalid depth {:?} received during fetch command args parsing",
                            depth
                        )
                    })?);
                } else if let Some(time_depth) = data.strip_prefix(DEEPEN_SINCE_PREFIX) {
                    let time_depth = bytes_to_str(time_depth, "depth", "deepen since")?.to_owned();
                    let parsed_time = gix_date::parse(time_depth.as_str(), Some(std::time::SystemTime::now()))
                        .with_context(|| format!("Invalid time {:?} received for deepen since during fetch command args parsing", time_depth))?;
                    fetch_args.deepen_since = Some(parsed_time);
                } else if let Some(oid_depth) = data.strip_prefix(DEEPEN_NOT_PREFIX) {
                    fetch_args.deepen_not = Some(parse_oid(oid_depth, DEEPEN_NOT_PREFIX)?);
                } else if let Some(filter) = data.strip_prefix(FILTER_PREFIX) {
                    let filter_spec = bytes_to_str(filter, "filter_spec", "filter")?.to_owned();
                    fetch_args.filter = Some(filter_spec);
                } else if let Some(want_ref) = data.strip_prefix(WANT_REF_PREFIX) {
                    let want_ref = bytes_to_str(want_ref, "want_ref", "want-ref")?.to_owned();
                    fetch_args.want_refs.push(want_ref);
                } else if let Some(packfile_uris) = data.strip_prefix(PACKFILE_URIS_PREFIX) {
                    let packfile_uris =
                        bytes_to_str(packfile_uris, "packfile_uris", "packfile-uris")?;
                    fetch_args.packfile_uris = Vec::from_iter(
                        packfile_uris
                            .split(PACKFILE_URIS_SEPARATOR)
                            .map(String::from),
                    );
                } else {
                    match data {
                        DONE => fetch_args.done = true,
                        THIN_PACK => fetch_args.thin_pack = true,
                        NO_PROGRESS => fetch_args.no_progress = true,
                        INCLUDE_TAG => fetch_args.include_tag = true,
                        OFSET_DELTA => fetch_args.ofs_delta = true,
                        WAIT_FOR_DONE => fetch_args.wait_for_done = true,
                        SIDEBAND_ALL => fetch_args.sideband_all = true,
                        DEEPEN_RELATIVE => fetch_args.deepen_relative = true,
                        arg => anyhow::bail!(
                            "Unexpected arg {} in fetch command args",
                            String::from_utf8_lossy(arg)
                        ),
                    };
                }
            } else {
                anyhow::bail!(
                    "Unexpected token {:?} in packetline during fetch command args parsing",
                    token
                );
            };
        }
        fetch_args.validate()?;
        Ok(fetch_args)
    }

    /// Convert the fetch command args into FetchRequest instance
    pub fn into_request(self, concurrency: PackfileConcurrency) -> FetchRequest {
        FetchRequest {
            heads: self.wants,
            bases: self.haves,
            include_out_of_pack_deltas: self.thin_pack,
            include_annotated_tags: self.include_tag,
            offset_delta: self.ofs_delta,
            shallow: self.shallow,
            deepen: self.deepen,
            deepen_since: self.deepen_since,
            deepen_not: self.deepen_not,
            deepen_relative: self.deepen_relative,
            filter: self.filter,
            concurrency,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use gix_packetline::encode::flush_to_write;
    use gix_packetline::Writer;

    use super::*;

    #[test]
    fn test_fetch_command_args_parsing() -> anyhow::Result<()> {
        let inner_writer = Vec::new();
        let mut packetline_writer = Writer::new(inner_writer);
        packetline_writer.write_all(b"thin-pack\n")?;
        packetline_writer.write_all(b"ofs-delta\n")?;
        packetline_writer.write_all(b"no-progress\n")?;
        packetline_writer.write_all(b"include-tag\n")?;
        packetline_writer.write_all(b"wait-for-done\n")?;
        packetline_writer.write_all(b"sideband-all\n")?;
        packetline_writer.write_all(b"shallow 0000000000000000000000000000000000000000\n")?;
        packetline_writer.write_all(b"deepen 1\n")?;
        packetline_writer.write_all(b"want-ref refs/heads/master\n")?;
        packetline_writer.write_all(b"want-ref refs/heads/release\n")?;
        packetline_writer.write_all(b"have 0000000000000000000000000000000000000000\n")?;
        packetline_writer.write_all(b"want 0000000000000000000000000000000000000000\n")?;
        packetline_writer.write_all(b"have 1000000000000000000000000000000000000001\n")?;
        packetline_writer.write_all(b"want 1000000000000000000000000000000000000001\n")?;
        packetline_writer.write_all(b"have 2000000000000000000000000000000000000002\n")?;
        packetline_writer.write_all(b"shallow 1000000000000000000000000000000000000001\n")?;
        packetline_writer.write_all(b"done\n")?;
        packetline_writer.flush()?;
        let mut inner_writer = packetline_writer.into_inner();
        flush_to_write(&mut inner_writer)?;

        let parsed_args = FetchArgs::parse_from_packetline(&inner_writer)?;
        assert!(parsed_args.thin_pack);
        assert!(parsed_args.ofs_delta);
        assert!(parsed_args.no_progress);
        assert!(parsed_args.include_tag);
        assert!(parsed_args.wait_for_done);
        assert!(parsed_args.sideband_all);
        assert!(parsed_args.done);
        assert_eq!(parsed_args.deepen, Some(1));
        assert_eq!(parsed_args.shallow.len(), 2);
        assert_eq!(parsed_args.haves.len(), 3);
        assert_eq!(parsed_args.wants.len(), 2);
        Ok(())
    }

    #[test]
    fn test_fetch_command_args_validation() -> anyhow::Result<()> {
        let inner_writer = Vec::new();
        let mut packetline_writer = Writer::new(inner_writer);
        packetline_writer.write_all(b"deepen 1\n")?;
        packetline_writer.write_all(b"deepen-since 1979-02-26 18:30:00\n")?;
        let mut inner_writer = packetline_writer.into_inner();
        flush_to_write(&mut inner_writer)?;

        assert!(FetchArgs::parse_from_packetline(&inner_writer).is_err());

        let inner_writer = Vec::new();
        let mut packetline_writer = Writer::new(inner_writer);
        packetline_writer.write_all(b"deepen 1\n")?;
        packetline_writer.write_all(b"deepen-not 1000000000000000000000000000000000000001\n")?;
        let mut inner_writer = packetline_writer.into_inner();
        flush_to_write(&mut inner_writer)?;

        assert!(FetchArgs::parse_from_packetline(&inner_writer).is_err());
        Ok(())
    }

    #[test]
    fn test_fetch_command_args_time_parsing() -> anyhow::Result<()> {
        let inner_writer = Vec::new();
        let mut packetline_writer = Writer::new(inner_writer);
        packetline_writer.write_all(b"deepen-since 1979-02-26 18:30:00\n")?;
        let mut inner_writer = packetline_writer.into_inner();
        flush_to_write(&mut inner_writer)?;

        assert!(FetchArgs::parse_from_packetline(&inner_writer).is_ok());

        let inner_writer = Vec::new();
        let mut packetline_writer = Writer::new(inner_writer);
        packetline_writer.write_all(b"deepen-since 10 weeks ago\n")?;
        let mut inner_writer = packetline_writer.into_inner();
        flush_to_write(&mut inner_writer)?;

        assert!(FetchArgs::parse_from_packetline(&inner_writer).is_ok());
        Ok(())
    }
}
