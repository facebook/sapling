/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

@rust.Exhaustive
struct ThriftStruct {
  1: i32 a;
  2: string b;
  3: i64 c;
  4: list<i32> d;
  5: ThriftSecondStruct e;
  6: list<ThriftSecondStruct> f;
}

@rust.Exhaustive
struct ThriftSecondStruct {
  1: i64 x;
  2: string y;
  3: optional string z;
}

union ThriftUnion {
  1: ThriftEmpty first;
  2: ThriftStruct second;
  3: ThriftSecondStruct third;
}

@rust.Exhaustive
struct ThriftEmpty {}
