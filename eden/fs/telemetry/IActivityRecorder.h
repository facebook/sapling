/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <functional>
#include <memory>
namespace facebook::eden {

class EdenMount;

class IActivityRecorder {
 public:
  explicit IActivityRecorder(std::shared_ptr<EdenMount> edenMount)
      : edenMount_{std::move(edenMount)} {}
  virtual ~IActivityRecorder() = default;
  virtual uint64_t addSubscriber() = 0;
  virtual void removeSubscriber(uint64_t unique) = 0;

 protected:
  std::shared_ptr<EdenMount> edenMount_;
};

class NullActivityRecorder : public IActivityRecorder {
 public:
  NullActivityRecorder() : IActivityRecorder{{}} {}
  uint64_t addSubscriber() override {
    return 0;
  }
  void removeSubscriber(uint64_t /* unique */) override {}
};

using ActivityRecorderFactory =
    std::function<std::unique_ptr<IActivityRecorder>(
        std::shared_ptr<EdenMount>)>;

} // namespace facebook::eden
