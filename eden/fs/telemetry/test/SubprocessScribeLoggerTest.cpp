/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/SubprocessScribeLogger.h"

#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using namespace folly::string_piece_literals;

TEST(ScribeLogger, log_messages_are_written_with_newlines) {
  folly::test::TemporaryFile output;

  {
    SubprocessScribeLogger logger{std::vector<std::string>{"/bin/cat"},
                                  output.fd()};
    logger.log("foo"_sp);
    logger.log("bar"_sp);
  }

  folly::checkUnixError(lseek(output.fd(), 0, SEEK_SET));
  std::string contents;
  folly::readFile(output.fd(), contents);
  EXPECT_EQ("foo\nbar\n", contents);
}
