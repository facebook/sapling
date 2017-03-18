/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

namespace folly {
class EventBaseManager;
}

namespace facebook { namespace fb303 {

class FacebookBase2 {
public:
  explicit FacebookBase2(const char*) {}

  void setEventBaseManager(folly::EventBaseManager*) {}
};

}}
