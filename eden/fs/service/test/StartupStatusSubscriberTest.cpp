/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/StartupStatusSubscriber.h"

#include <memory>

#include <folly/portability/GTest.h>

#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {

class SimpleStartupStatusSubscriber : public StartupStatusSubscriber {
 public:
  SimpleStartupStatusSubscriber(
      std::vector<std::string_view>& publishList,
      uint32_t& completeCount)
      : publishList_{publishList}, completeCount_{completeCount} {}

  ~SimpleStartupStatusSubscriber() noexcept override {
    completeCount_++;
  }

  void publish(std::string_view data) override {
    publishList_.push_back(data);
  }

  std::vector<std::string_view>& publishList_;
  uint32_t& completeCount_;
};

TEST(StartupStatusChannel, createAndComplete) {
  StartupStatusChannel state;
  state.startupCompleted();
}

TEST(StartupStatusChannel, noSubscriberPublish) {
  StartupStatusChannel state;
  state.publish("blah");
  state.startupCompleted();
}

TEST(StartupStatusChannel, noSubscriberPublishAfterComplete) {
  StartupStatusChannel state;
  state.startupCompleted();
  state.publish("blah");
}

TEST(StartupStatusChannel, addSubscriber) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);

  StartupStatusChannel state;
  state.subscribe(std::move(subscriber));
  state.publish("blah");
  state.startupCompleted();

  EXPECT_EQ(1, publishList.size());
  EXPECT_EQ("blah", publishList.at(0));
  EXPECT_EQ(1, completeCount);
}

TEST(StartupStatusChannel, add2Subscriber) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber1 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);
  auto subscriber2 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);

  StartupStatusChannel state;
  state.subscribe(std::move(subscriber1));
  state.subscribe(std::move(subscriber2));
  state.publish("blah");
  state.startupCompleted();

  EXPECT_EQ(2, publishList.size());
  EXPECT_EQ("blah", publishList.at(0));
  EXPECT_EQ("blah", publishList.at(1));
  EXPECT_EQ(2, completeCount);
}

TEST(StartupStatusChannel, addSubscriberAfterPublish) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber1 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);
  auto subscriber2 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);

  StartupStatusChannel state;
  state.subscribe(std::move(subscriber1));
  state.publish("blah");
  state.subscribe(std::move(subscriber2));
  state.startupCompleted();

  EXPECT_EQ(1, publishList.size());
  EXPECT_EQ("blah", publishList.at(0));
  EXPECT_EQ(2, completeCount);
}

TEST(StartupStatusChannel, publishAfterCompleteWithSubscriber) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber1 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);
  auto subscriber2 = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);

  StartupStatusChannel state;
  state.subscribe(std::move(subscriber1));
  state.subscribe(std::move(subscriber2));
  state.startupCompleted();
  state.publish("blah");

  EXPECT_EQ(0, publishList.size());
  EXPECT_EQ(2, completeCount);
}

TEST(StartupStatusChannel, addSubscriberAfterComplete) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);

  StartupStatusChannel state;
  state.publish("blah");
  state.startupCompleted();
  EXPECT_THROW(state.subscribe(std::move(subscriber)), EdenError);
  state.publish("blah2");

  EXPECT_EQ(0, publishList.size());
  EXPECT_EQ(1, completeCount);
}

TEST(StartupStatusChannel, stateDestroyedWithoutComplete) {
  std::vector<std::string_view> publishList;
  uint32_t completeCount = 0;
  auto subscriber = std::make_unique<SimpleStartupStatusSubscriber>(
      publishList, completeCount);
  {
    StartupStatusChannel state;
    state.subscribe(std::move(subscriber));
    state.publish("blah");
    // subscribers will be destructed when the state is destructed
  }

  EXPECT_EQ(1, publishList.size());
  EXPECT_EQ("blah", publishList.at(0));
  EXPECT_EQ(1, completeCount);
}

} // namespace facebook::eden
