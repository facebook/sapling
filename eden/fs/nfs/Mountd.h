/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// Implementation of the mount protocol as described in:
// https://tools.ietf.org/html/rfc1813#page-106

#include "eden/fs/nfs/rpc/Server.h"

namespace facebook::eden {

class Mountd {
 public:
  Mountd();

  Mountd(const Mountd&) = delete;
  Mountd(Mountd&&) = delete;
  Mountd& operator=(const Mountd&) = delete;
  Mountd& operator=(Mountd&&) = delete;

 private:
  RpcServer server_;
};

} // namespace facebook::eden
