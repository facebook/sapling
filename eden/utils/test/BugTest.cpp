/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/utils/Bug.h"

#include <folly/ExceptionWrapper.h>
#include <gtest/gtest.h>
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;

namespace {
void buggyFunction() {
  EDEN_BUG() << "oh noes";
}
}

TEST(EdenBug, throws) {
  EdenBugDisabler noCrash;
  EXPECT_THROW_RE(buggyFunction(), std::runtime_error, "oh noes");
  EXPECT_THROW_RE(EDEN_BUG() << "doh", std::runtime_error, "doh");
}

TEST(EdenBug, toException) {
  EdenBugDisabler noCrash;
  auto bug = EDEN_BUG() << "whoops";
  auto ew = bug.toException();
  EXPECT_THROW_RE(ew.throwException(), std::runtime_error, "whoops");
}
