/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftStreamStartupStatusSubscriber.h"

#include <memory>

#include <folly/portability/GTest.h>

// its not very easy to get the data out of a thrift stream, so we don't have
// any tests for this. We already test that correct data is published to
// subscribers. so these tests exist to test publisher lifetime and exception
// cases.

namespace facebook::eden {

TEST(ThriftStreamStartupStatusSubscriber, createAndCancel) {
  auto state = std::make_shared<StartupStatusChannel>();

  {
    auto stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
  } // delete the stream, this should cancel the publisher.

  state->startupCompleted();

  // ensure everything tears down nicely.
}

TEST(ThriftStreamStartupStatusSubscriber, createAndComplete) {
  auto state = std::make_shared<StartupStatusChannel>();

  {
    auto stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
    state->startupCompleted(); // complete the publisher, this should destroy
                               // it
  } // delete the stream, this would cancel the publisher, but it should
  // already be deleted at this point, so make sure we don't hit any snags.

  // ensure everything tears down nicely.
}

TEST(ThriftStreamStartupStatusSubscriber, publishAndComplete) {
  auto state = std::make_shared<StartupStatusChannel>();

  {
    auto stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
    state->publish("blah");
    state->startupCompleted();
  }

  // ensure everything tears down nicely.
}

TEST(ThriftStreamStartupStatusSubscriber, publishAndCancel) {
  auto state = std::make_shared<StartupStatusChannel>();

  {
    auto stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
    state->publish("blah");
  } // delete the stream, this should cancel the publisher.

  state->startupCompleted(); // this should complete and destroy the publisher.

  // ensure everything tears down nicely.
}

TEST(ThriftStreamStartupStatusSubscriber, publishAfterCancel) {
  auto state = std::make_shared<StartupStatusChannel>();

  {
    auto stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
  } // delete the stream, this should cancel the publisher.
  state->publish("blah");
  state->startupCompleted();

  // ensure everything tears down nicely.
}

TEST(ThriftStreamStartupStatusSubscriber, forgetToComplete) {
  std::optional<apache::thrift::ServerStream<std::string>> stream;
  {
    auto state = std::make_shared<StartupStatusChannel>();
    stream =
        ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
            state);
  } // forgot to startupCompleted ensure things don't blow up when the state and
    // therefore publisher gets cleaned up.
}

} // namespace facebook::eden
