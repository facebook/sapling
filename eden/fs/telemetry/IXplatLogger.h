/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string_view>

namespace facebook::eden {

class DynamicEvent;

class IXplatLogger {
 public:
  virtual ~IXplatLogger() = default;

  virtual void logEvent(
      std::string_view category,
      const DynamicEvent& event) = 0;
};

class NullXplatLogger : public IXplatLogger {
 public:
  void logEvent(std::string_view /*category*/, const DynamicEvent& /*event*/)
      override {}
};

} // namespace facebook::eden
