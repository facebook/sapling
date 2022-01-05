/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use minibytes::Bytes;

/// TODO(T48685378): Handle redacted content in a less hacky way.
pub static REDACTED_CONTENT: &[u8] = b"PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";
static REDACTED_MESSAGE: &[u8] = b"This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.\n";

pub fn is_redacted(data: &Bytes) -> bool {
    data.as_ref() == REDACTED_CONTENT
}

pub fn redact_if_needed(data: Bytes) -> Bytes {
    if is_redacted(&data) {
        Bytes::from_static(REDACTED_MESSAGE)
    } else {
        data
    }
}
