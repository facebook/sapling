/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/win/mount/FsChannel.h"

namespace facebook {
namespace eden {

class TestFsChannel : public FsChannel {
 public:
  TestFsChannel(const TestFsChannel&) = delete;
  TestFsChannel& operator=(const TestFsChannel&) = delete;

  TestFsChannel(){};
  virtual ~TestFsChannel() = default;
  virtual void start() override {}
  virtual void stop() override {}
};

} // namespace eden
} // namespace facebook
//////////////////////////
