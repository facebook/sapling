/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_types::{typed_hash, ContentChunkId, ContentId};

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
