/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;

pub static REDACTED_CONTENT: Bytes = Bytes::from_static("PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n".as_bytes());
static REDACTED_MESSAGE: Bytes = Bytes::from_static("This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.\n".as_bytes());

pub fn is_redacted(data: &Bytes) -> bool {
    *data == REDACTED_CONTENT
}

pub fn redact_if_needed(data: Bytes) -> Bytes {
    if is_redacted(&data) {
        REDACTED_MESSAGE.clone()
    } else {
        data
    }
}
