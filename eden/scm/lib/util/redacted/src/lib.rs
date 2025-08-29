/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use blob::Blob;
use blob::Bytes;
use types::Sha256;

/// TODO(T48685378): Handle redacted content in a less hacky way.
pub static REDACTED_CONTENT: &[u8] = b"PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

pub static REDACTED_SHA256: Sha256 = Sha256::from_byte_array([
    237, 197, 122, 52, 176, 2, 147, 212, 153, 24, 58, 188, 175, 183, 88, 59, 27, 75, 172, 22, 11,
    247, 139, 29, 211, 97, 111, 104, 65, 129, 23, 140,
]);

static REDACTED_MESSAGE: &[u8] = b"This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.\n";

pub fn is_redacted(data: &Blob) -> bool {
    data == &Blob::from(Bytes::from_static(REDACTED_CONTENT))
}

pub fn redact_if_needed(data: Bytes) -> Bytes {
    if is_redacted(&data.clone().into()) {
        Bytes::from_static(REDACTED_MESSAGE)
    } else {
        data
    }
}
