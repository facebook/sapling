/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

union InChunk {
  1: i32 num_of_chunks,
}

union DataCacheEntry {
  1: list<byte> data,
  2: InChunk in_chunk,
}
