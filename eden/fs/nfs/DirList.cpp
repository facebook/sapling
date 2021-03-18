/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/DirList.h"

namespace facebook::eden {

namespace {

/**
 * Overhead of READDIR3resok before adding any entries:
 * - A filled post_op_attr
 * - The cookieverf,
 * - The eof boolean and the end of the list marker.
 */
constexpr size_t kInitialOverhead =
    XdrTrait<post_op_attr>::serializedSize(post_op_attr{fattr3{}}) +
    XdrTrait<uint64_t>::serializedSize(0) +
    2 * XdrTrait<bool>::serializedSize(false);

/**
 * NFS is weird, it specifies the maximum amount of entries to be returned by
 * passing the total size of the READDIR3resok structure, therefore we need to
 * account for all the overhead.
 */
uint32_t computeInitialRemaining(uint32_t count) {
  if (kInitialOverhead > count) {
    throw std::length_error(
        "NFS READDIR overhead is bigger than the passed in size");
  }
  return count - kInitialOverhead;
}
} // namespace

NfsDirList::NfsDirList(uint32_t count)
    : remaining_(computeInitialRemaining(count)) {}

bool NfsDirList::add(
    folly::StringPiece name,
    InodeNumber ino,
    uint64_t offset) {
  auto entry = entry3{ino.get(), name.str(), offset};
  // The serialized size includes a boolean indicating that this is not the end
  // of the list.
  auto neededSize = XdrTrait<entry3>::serializedSize(entry) +
      XdrTrait<bool>::serializedSize(true);

  if (neededSize > remaining_) {
    return false;
  }

  remaining_ -= neededSize;
  list_.list.push_back(std::move(entry));
  return true;
}

} // namespace facebook::eden

#endif
