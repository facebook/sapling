/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/SubprocessScribeLogger.h"

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>

using namespace facebook::eden;
using namespace folly::string_piece_literals;

TEST(ScribeLogger, log_messages_are_written_with_newlines) {
  folly::test::TemporaryFile output;

  {
    SubprocessScribeLogger logger{
        std::vector<std::string>{"/bin/cat"},
        FileDescriptor(
            ::dup(output.fd()), "dup", FileDescriptor::FDType::Generic)};
    logger.log("foo"_sp);
    logger.log("bar"_sp);
  }

  folly::checkUnixError(lseek(output.fd(), 0, SEEK_SET));
  std::string contents;
  folly::readFile(output.fd(), contents);
  EXPECT_EQ("foo\nbar\n", contents);
}
