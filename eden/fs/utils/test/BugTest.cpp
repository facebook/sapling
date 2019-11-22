/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Bug.h"

#include <folly/ExceptionWrapper.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

namespace {
void buggyFunction() {
  EDEN_BUG() << "oh noes";
}
} // namespace

TEST(EdenBug, throws) {
  EdenBugDisabler noCrash;
  EXPECT_THROW_RE(buggyFunction(), std::runtime_error, "oh noes");
  EXPECT_THROW_RE(EDEN_BUG() << "doh", std::runtime_error, "doh");
}

TEST(EdenBug, toException) {
  EdenBugDisabler noCrash;
  auto ew = EDEN_BUG_EXCEPTION() << "whoops";
  EXPECT_THROW_RE(ew.throw_exception(), std::runtime_error, "whoops");
}
