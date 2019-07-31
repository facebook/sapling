// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mononoke_types::{typed_hash, ContentId};

use super::StoreRequest;

mod failing_blobstore;
mod test_api;
mod test_invariants;

fn request(data: impl AsRef<[u8]>) -> StoreRequest {
    StoreRequest::new(data.as_ref().len() as u64)
}

fn canonical(data: impl AsRef<[u8]>) -> ContentId {
    let mut ctx = typed_hash::ContentIdContext::new();
    ctx.update(data.as_ref());
    ctx.finish()
}
