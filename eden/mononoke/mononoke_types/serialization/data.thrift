/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

include "thrift/annotation/rust.thrift"

/// Binary data that may be large, stored in `Bytes`.
@rust.Type{name = "Bytes"}
typedef binary LargeBinary

/// Binary data that is likely small, and stored inline using `SmallVec`.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 24]>"}
typedef binary SmallBinary
