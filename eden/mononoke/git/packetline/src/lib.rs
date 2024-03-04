/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Size of the length prefix in bytes for each packetline line
const U16_HEX_BYTES: usize = 4;
/// Maximum number of bytes in a single packetline line excluding the length prefix
const MAX_DATA_LEN: usize = 65516;
/// Packetline representing the end of a message
pub const FLUSH_LINE: &[u8] = b"0000";
/// Packetline separating sections of a message
pub const DELIMITER_LINE: &[u8] = b"0001";
/// Packetline representing the end of response for stateless connections
const RESPONSE_END_LINE: &[u8] = b"0002";
/// Prefix for error messages
const ERR_PREFIX: &[u8] = b"ERR ";

pub mod encode;

/// One of three side-band types allowing to multiplex information over a single connection.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum Channel {
    /// The usable data itself in any format.
    Data = 1,
    /// Progress information in a user-readable format.
    Progress = 2,
    /// Error information in a user readable format. Receiving it usually terminates the connection.
    Error = 3,
}
