// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use censoredblob::ErrorKind::Censored;
use failure::Error;
use futures::{future, Future};
use futures_ext::FutureExt;
use mercurial_types::{FileBytes, HgBlob, RevFlags};

/// Tombstone string to replace the content of blacklisted files with
const CENSORED_CONTENT: &str =
    "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

/// A helper to replace censorship erros with tombstone structs
/// TODO(T48685378): Handle redacted content in a less hacky way
pub fn hide_censorship_error<I>(
    f: impl Future<Item = I, Error = Error>,
    tombstone_factory: impl FnOnce() -> I,
) -> impl Future<Item = I, Error = Error> {
    f.or_else(move |err| {
        let root_cause = err.find_root_cause();
        let maybe_censored_err = root_cause.downcast_ref::<censoredblob::ErrorKind>();

        // if the error is Censored return a tombstone as the new content
        match maybe_censored_err {
            Some(Censored(_, _)) => future::ok(tombstone_factory()).right_future(),
            None => future::err(err).left_future(),
        }
    })
}

pub fn tombstone_filebytes_revflags() -> (FileBytes, RevFlags) {
    (
        FileBytes(CENSORED_CONTENT.as_bytes().into()),
        RevFlags::REVIDX_DEFAULT_FLAGS,
    )
}

pub fn tombstone_hgblob() -> HgBlob {
    HgBlob::from(CENSORED_CONTENT.as_bytes().to_vec())
}
