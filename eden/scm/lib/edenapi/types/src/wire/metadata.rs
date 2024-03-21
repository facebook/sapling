/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use crate::metadata::WireAnyFileContentId;
pub use crate::metadata::WireDirectoryMetadata;
pub use crate::metadata::WireFileMetadata;
pub use crate::metadata::WireFileType;
use crate::Blake3;
use crate::ContentId;
use crate::FsnodeId;
use crate::Sha1;
use crate::Sha256;
use crate::ToApi;
use crate::ToWire;

wire_hash! {
    wire => WireFsnodeId,
    api  => FsnodeId,
    size => 32,
}

wire_hash! {
    wire => WireContentId,
    api  => ContentId,
    size => 32,
}

wire_hash! {
    wire => WireSha1,
    api  => Sha1,
    size => 20,
}

wire_hash! {
    wire => WireSha256,
    api  => Sha256,
    size => 32,
}

wire_hash! {
    wire => WireBlake3,
    api => Blake3,
    size => 32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireFileMetadata,
        WireDirectoryMetadata,
        WireAnyFileContentId
    );
}
