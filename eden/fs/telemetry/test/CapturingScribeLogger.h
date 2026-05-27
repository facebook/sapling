/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>

#include "eden/common/telemetry/ScribeLogger.h"

namespace facebook::eden {

/**
 * A ScribeLogger that captures messages for test verification.
 */
class CapturingScribeLogger : public ScribeLogger {
 public:
  void log(std::string message) override {
    messages_.push_back(std::move(message));
  }

  const std::vector<std::string>& messages() const {
    return messages_;
  }

 private:
  std::vector<std::string> messages_;
};

} // namespace facebook::eden
