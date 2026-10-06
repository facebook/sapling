/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

include "thrift/annotation/rust.thrift"

package "meta.com/thrift/rust/test/server_args"

enum Color {
  RED = 1,
  GREEN = 2,
}

struct Inner {
  1: i32 plain;
  2: i32 with_default = 7;
}

union Choice {
  1: i32 number;
  2: string text;
}

@rust.Adapter{name = "::adapters::IdentityAdapter<>"}
typedef Inner AdaptedInner

@rust.NewType
typedef AdaptedInner WrappedAdaptedInner

@rust.Adapter{name = "::adapters::NonZeroI64Adapter"}
typedef i64 NonZeroId

service ServerArgs {
  void echo(
    1: i32 number,
    2: string text,
    3: list<i64> numbers,
    4: Inner inner,
    5: Color color,
    6: Choice choice,
    7: AdaptedInner adapted,
  );
  void non_zero(10: NonZeroId id);
  void wrapped(11: WrappedAdaptedInner wrapped);
}

// Same field ids and wire types as the arguments of every `ServerArgs` method,
// but optional, so a test chooses which arguments a request carries.
struct PartialArgs {
  1: optional i32 number;
  2: optional string text;
  7: optional Inner adapted;
  10: optional i64 id;
  11: optional Inner wrapped;
}
