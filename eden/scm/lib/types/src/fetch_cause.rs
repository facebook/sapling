/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FetchCause {
    // Unknown orginination from EdenFS
    EdenUnknown,
    // The fetch originated from a Eden Thrift prefetch endpoint
    EdenPrefetch,
    // The fetch originated from a Eden Thrift endpoint
    EdenThrift,
    // The fetch originated from FUSE/NFS/PrjFS
    EdenFs,
    // The fetch originated from a mixed EdenFS causes
    EdenMixed,
    // The fetch originated from a Sapling prefetch
    SaplingPrefetch,
    // Unknown orginination from Sapling
    SaplingUnknown,
    // Unknown originiation, usually from Sapling (the default)
    Unspecified,
}
