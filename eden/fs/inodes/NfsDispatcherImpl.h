/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include "eden/fs/nfs/NfsDispatcher.h"

namespace facebook::eden {
class EdenMount;
class InodeMap;

class NfsDispatcherImpl : public NfsDispatcher {
 public:
  explicit NfsDispatcherImpl(EdenMount* mount);

  folly::Future<struct stat> getattr(
      InodeNumber ino,
      ObjectFetchContext& context) override;

  folly::Future<InodeNumber> getParent(
      InodeNumber ino,
      ObjectFetchContext& context) override;

  folly::Future<std::tuple<InodeNumber, struct stat>> lookup(
      InodeNumber dir,
      PathComponent name,
      ObjectFetchContext& context) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;
  InodeMap* const inodeMap_;
};
} // namespace facebook::eden

#endif
