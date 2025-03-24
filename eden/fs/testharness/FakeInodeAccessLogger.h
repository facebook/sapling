/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/InodeAccessLogger.h"

namespace facebook::eden {
/**
 * A fake implementation of InodeAccessLogger that just counts the number of
 * accesses per EdenMount. This is useful for testing to ensure that we logging
 * the expected number of accesses.
 */
class FakeInodeAccessLogger : public InodeAccessLogger {
 public:
  FakeInodeAccessLogger() : InodeAccessLogger(nullptr, nullptr) {}

  virtual void logInodeAccess(InodeAccess) override {
    ++accessCount_;
  }

  void reset() {
    accessCount_ = 0;
  }

  size_t getAccessCount() const {
    return accessCount_;
  }

 private:
  size_t accessCount_{0};
};
} // namespace facebook::eden
