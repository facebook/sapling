/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook {
namespace eden {

class FsChannel {
 public:
  FsChannel(const FsChannel&) = delete;
  FsChannel& operator=(const FsChannel&) = delete;

  FsChannel(){};
  virtual ~FsChannel() = default;
  virtual void start() = 0;
  virtual void stop() = 0;

  virtual void removeCachedFile(const wchar_t* path) = 0;
  virtual void removeDeletedFile(const wchar_t* path) = 0;
};

} // namespace eden
} // namespace facebook
//////////////////////////
