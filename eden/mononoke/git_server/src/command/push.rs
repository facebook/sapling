/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::from_utf8;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use bytes::Bytes;
use gix_hash::ObjectId;
use gix_packetline::PacketLineRef;
use gix_packetline::StreamingPeekableIter;

use super::PUSH_MARKER;
use super::fetch::parse_oid;

const REPORT_STATUS: &str = "report-status";
const OBJECT_FORMAT: &str = "object-format=";
const ATOMIC: &str = "atomic";
const DELETE_REFS: &str = "delete-refs";
const QUIET: &str = "quiet";
const SHALLOW_PREFIX: &[u8] = b"shallow";

/// Enum representing the object format for hashes
#[derive(Clone, Debug, Copy)]
pub enum ObjectFormat {
    /// Use Sha1 hashes for Git objects
    Sha1,
    /// Use Sha256 hashes for Git objects
    Sha256,
}

impl ObjectFormat {
    fn parse(format: &str) -> Result<Self> {
        match format {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            format => bail!("Invalid object format: {}", format),
        }
    }
}

impl Default for ObjectFormat {
    fn default() -> Self {
        Self::Sha1
    }
}

/// Enum representing the type of ref that is being pushed
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum RefType {
    /// The type of ref is not determined
    Unknown,
    /// Variant representing ref pointing to a commit or tag
    Standard,
    /// Variant representing ref pointing to blob or tree
    Content,
}

impl From<gix_object::Kind> for RefType {
    fn from(kind: gix_object::Kind) -> Self {
        match kind {
            gix_object::Kind::Commit | gix_object::Kind::Tag => Self::Standard,
            gix_object::Kind::Tree | gix_object::Kind::Blob => Self::Content,
        }
    }
}

/// Struct representing the move of a ref/bookmark from "from" commit ID to "to" commit ID
#[derive(Clone, Debug)]
pub struct RefUpdate {
    pub ref_name: String,
    pub ref_type: RefType,
    pub from: ObjectId,
    pub to: ObjectId,
}

impl RefUpdate {
    pub fn new(ref_name: String, ref_type: RefType, from: ObjectId, to: ObjectId) -> Self {
        Self {
            ref_name,
            ref_type,
            from,
            to,
        }
    }

    fn parse(input: &[u8]) -> Result<Self> {
        let parsed_input =
            from_utf8(input).context("Failure in converting ref-update line into UTF-8 string")?;
        let mut parts = parsed_input.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(from), Some(to), Some(ref_name)) => {
                let from_oid = parse_oid(from.as_bytes(), b"from ")?;
                let to_oid = parse_oid(to.as_bytes(), b"to ")?;
                Ok(Self {
                    ref_name: ref_name.to_string(),
                    ref_type: RefType::Unknown,
                    from: from_oid,
                    to: to_oid,
                })
            }
            split_parts => bail!("Invalid format for ref-update: {:?}", split_parts),
        }
    }

    pub fn is_content(&self) -> bool {
        self.ref_type == RefType::Content
    }
}

/// Struct representing the setting to be utilized during the push operation at the server
#[derive(Clone, Debug)]
pub struct PushSettings {
    /// When specified, the client needs the server to provide summary information about the result
    /// of the push operation per ref/branch/bookmark in the output
    pub report_status: bool,
    /// When specified, the server needs to delete the refs that the client provided with a 0 hex value
    pub delete_refs: bool,
    /// When specified, the server needs to apply the push for all refs/bookmarks atomically
    pub atomic: bool,
    /// When specified the server should only provide push status and refrain from adding any progress
    /// output
    pub quiet: bool,
    /// The format of the object hashes, defaults to sha1
    pub object_format: ObjectFormat,
}

impl PushSettings {
    fn parse(input: &[u8]) -> Result<Self> {
        let mut settings = Self::default();
        let parsed_input =
            from_utf8(input).context("Failure in converting push settings into UTF-8 string")?;
        for setting in parsed_input.split_whitespace() {
            if let Some(object_format) = setting.strip_prefix(OBJECT_FORMAT) {
                settings.object_format = ObjectFormat::parse(object_format)?;
            }
            match setting {
                REPORT_STATUS => settings.report_status = true,
                ATOMIC => settings.atomic = true,
                DELETE_REFS => settings.delete_refs = true,
                QUIET => settings.quiet = true,
                _ => {}
            };
        }
        Ok(settings)
    }
}

impl Default for PushSettings {
    fn default() -> Self {
        Self {
            object_format: ObjectFormat::default(),
            report_status: true,
            delete_refs: false,
            atomic: false,
            quiet: false,
        }
    }
}

/// Arguments for `ls-refs` command
#[derive(Clone, Debug, Default)]
pub struct PushArgs {
    /// The settings that would be utilized during the push
    pub settings: PushSettings,
    /// The bytes of the packfile to be pushed
    pub pack_file: std::io::Cursor<Bytes>,
    /// List of ref moves/updates that are part of the user push
    pub ref_updates: Vec<RefUpdate>,
    /// List of shallow commits that are part of the user push
    pub shallow: Vec<ObjectId>,
}

impl PushArgs {
    pub fn parse_from_packetline(args: Bytes) -> Result<Self> {
        let mut push_args = Self::default();
        let args = std::io::Cursor::new(args);
        let mut tokens = StreamingPeekableIter::new(args, &[PacketLineRef::Flush], true);
        while let Some(token) = tokens.read_line() {
            let token =
                token.context("Failed to read line from packetline during push arg parsing")??;
            if let PacketLineRef::Data(data) = token {
                if data.starts_with(SHALLOW_PREFIX) {
                    let parsed_input = from_utf8(data)
                        .context("Failure in converting shallow line into UTF-8 string")?;
                    let mut parts = parsed_input.split_whitespace();
                    if let (Some("shallow"), Some(oid)) = (parts.next(), parts.next()) {
                        push_args
                            .shallow
                            .push(parse_oid(oid.as_bytes(), b"shallow ")?);
                    } else {
                        bail!("Invalid format for shallow: {:?}", parsed_input);
                    }
                    continue;
                }
                let mut parts = data.split(|elem| elem == &PUSH_MARKER);
                match (parts.next(), parts.next()) {
                    (Some(ref_update), Some(push_settings)) => {
                        push_args.ref_updates.push(RefUpdate::parse(ref_update)?);
                        push_args.settings = PushSettings::parse(push_settings)?;
                    }
                    (Some(ref_update), None) => {
                        push_args.ref_updates.push(RefUpdate::parse(ref_update)?);
                    }
                    _ => bail!("Incorrect format for push args: {:?}", from_utf8(data)),
                }
            } else {
                bail!(
                    "Unexpected token {:?} in packetline during push arg parsing",
                    token
                );
            }
        }
        push_args.pack_file = tokens.into_inner();
        Ok(push_args)
    }

    pub fn is_shallow(&self) -> bool {
        !self.shallow.is_empty()
    }
}
