/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
 * Hardcoded static overhead of READDIR3resok before adding any entries.
 * Ideally we should make XdrTrait<T>::serializedSize a constexpr, but some
 * compiler (gcc) don't seem to be able to then compile the code. Thus, this
 * value is hardcoded here and verified in the DirListTest.
 */
constexpr size_t kNfsDirListInitialOverhead = 104u;

/**
 * Abstraction to only add as many directory entries that can fit into a given
 * amount of memory.
 */
class NfsDirList {
 public:
  explicit NfsDirList(uint32_t count, nfsv3Procs listType);

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
  template <typename T>
  XdrList<T> extractList() {
    return std::get<XdrList<T>>(std::move(list_));
  }

  /**
   * Access the built list via reference. Only implemented for entryplus3
   * since only the entryplus3 implementation will need to fill in stat data.
   *
   * Use this method when you need to modify the NfsDirList's vector of entries.
   */
  std::vector<entryplus3>& getListRef() {
    return std::get<XdrList<entryplus3>>(list_).list;
  }

 private:
  uint32_t remaining_;
  std::variant<XdrList<entry3>, XdrList<entryplus3>> list_{};
};

} // namespace facebook::eden

#endif
