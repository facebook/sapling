/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::typed_hash;
use mononoke_types::ContentChunkId;
use mononoke_types::ContentId;

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

fn chunk(data: impl AsRef<[u8]>) -> ContentChunkId {
    let mut ctx = typed_hash::ContentChunkIdContext::new();
    ctx.update(data.as_ref());
    ctx.finish()
}
