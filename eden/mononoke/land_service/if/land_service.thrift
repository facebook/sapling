/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"

struct FooRequest {
  1: string bar;
}

struct FooResult {
  1: bool is_valid;
}

exception FooExn {
  1: string reason;
} (message = 'reason')

service LandService extends fb303_core.BaseService {
  # TODO: Add thrift functions here, and then implement them in the land_service_impl.rs file!
  FooResult validateFoo(1: FooRequest inputs) throws (1: FooExn ex);
}
