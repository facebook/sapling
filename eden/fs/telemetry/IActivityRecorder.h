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

 protected:
  std::shared_ptr<EdenMount> edenMount_;
};

class NullActivityRecorder : public IActivityRecorder {
 public:
  NullActivityRecorder() : IActivityRecorder{{}} {}
};

using ActivityRecorderFactory =
    std::function<std::unique_ptr<IActivityRecorder>(
        std::shared_ptr<EdenMount>)>;

} // namespace facebook::eden
