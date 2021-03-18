/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/nfs/NfsdRpc.h"

namespace facebook::eden {

/**
 * Abstraction to only add as many directory entries that can fit into a given
 * amount of memory.
 */
class NfsDirList {
 public:
  explicit NfsDirList(uint32_t count);

  NfsDirList(NfsDirList&&) = default;
  NfsDirList& operator=(NfsDirList&&) = default;

  NfsDirList() = delete;
  NfsDirList(const NfsDirList&) = delete;
  NfsDirList& operator=(const NfsDirList&) = delete;

  /**
   * Add an entry. Return true if the entry was successfully added, false
   * otherwise.
   */
  bool add(folly::StringPiece name, InodeNumber ino, uint64_t offset);

  /**
   * Move the built list out of the NfsDirList.
   */
  XdrList<entry3> extractList() {
    return std::move(list_);
  }

 private:
  uint32_t remaining_;
  XdrList<entry3> list_{};
};

} // namespace facebook::eden

#endif
