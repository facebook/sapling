/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <functional>
#include <memory>
#include <optional>
#include "eden/fs/utils/PathFuncs.h"
namespace facebook::eden {

class EdenMount;

class IActivityRecorder {
 public:
  explicit IActivityRecorder(std::shared_ptr<EdenMount> edenMount)
      : edenMount_{std::move(edenMount)} {}
  virtual ~IActivityRecorder() = default;
  virtual uint64_t addSubscriber(AbsolutePathPiece outputPath) = 0;
  virtual std::optional<std::string> removeSubscriber(uint64_t unique) = 0;
  virtual std::vector<std::tuple<uint64_t, std::string>> getSubscribers() = 0;

 protected:
  std::shared_ptr<EdenMount> edenMount_;
};

class NullActivityRecorder : public IActivityRecorder {
 public:
  NullActivityRecorder() : IActivityRecorder{{}} {}
  uint64_t addSubscriber(AbsolutePathPiece /* outputPath */) override {
    return 0;
  }
  std::optional<std::string> removeSubscriber(uint64_t /* unique */) override {
    return std::nullopt;
  }
  std::vector<std::tuple<uint64_t, std::string>> getSubscribers() override {
    return {};
  }
};

using ActivityRecorderFactory =
    std::function<std::unique_ptr<IActivityRecorder>(
        std::shared_ptr<EdenMount>)>;

} // namespace facebook::eden
