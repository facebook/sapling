/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/telemetry/IXplatLogger.h"

namespace facebook::eden {

struct CapturedXplatEvent {
  std::string category;
  DynamicEvent event;
};

/**
 * An IXplatLogger that captures events for test verification.
 */
class CapturingXplatLogger : public IXplatLogger {
 public:
  void logEvent(std::string_view category, const DynamicEvent& event) override {
    events_.push_back(CapturedXplatEvent{std::string{category}, event});
  }

  const std::vector<CapturedXplatEvent>& events() const {
    return events_;
  }

 private:
  std::vector<CapturedXplatEvent> events_;
};

} // namespace facebook::eden
