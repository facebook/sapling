/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/DirList.h"
#include <variant>
#include "eden/fs/nfs/NfsdRpc.h"

namespace facebook::eden {

namespace {

/**
 * NFS is weird, it specifies the maximum amount of entries to be returned by
 * passing the total size of the READDIR3resok structure, therefore we need to
 * account for all the overhead.
 */
uint32_t computeInitialRemaining(uint32_t count) {
  if (kNfsDirListInitialOverhead > count) {
    throw std::length_error(
        "NFS READDIR overhead is bigger than the passed in size");
  }
  return count - kNfsDirListInitialOverhead;
}

std::variant<XdrList<entry3>, XdrList<entryplus3>> computeListType(
    nfsv3Procs listType) {
  switch (listType) {
    case nfsv3Procs::readdirplus:
      return std::variant<XdrList<entry3>, XdrList<entryplus3>>(
          std::in_place_type<XdrList<entryplus3>>);
    case nfsv3Procs::readdir:
    default:
      return std::variant<XdrList<entry3>, XdrList<entryplus3>>(
          std::in_place_type<XdrList<entry3>>);
  }
}

} // namespace

NfsDirList::NfsDirList(uint32_t count, nfsv3Procs listType)
    : remaining_(computeInitialRemaining(count)),
      list_(computeListType(listType)) {}

bool NfsDirList::add(
    folly::StringPiece name,
    InodeNumber ino,
    uint64_t offset) {
  auto fn = [name, ino, offset](auto&& list, uint32_t& remainingSize) {
    size_t neededSize;
    using ListType = std::decay_t<decltype(list->list)>;
    using EntryT = typename ListType::value_type;

    // For entryplus3s, we initially add an empty post_op_attr. This is
    // because we don't have access to stat data in this layer. In a
    // separate layer, we will fill in the post_op_attr with the
    // appropriate stat data. For entry3s, we don't need this extra data.
    EntryT entry = EntryT{ino, name.str(), offset};

    // The serialized size includes a boolean indicating that this is not
    // the end of the list.
    neededSize = XdrTrait<EntryT>::serializedSize(entry) +
        XdrTrait<bool>::serializedSize(true);

    if (neededSize > remainingSize) {
      return false;
    }

    remainingSize -= neededSize;
    list->list.push_back(std::move(entry));
    return true;
  };

  if (XdrList<entryplus3>* list = std::get_if<XdrList<entryplus3>>(&list_)) {
    return fn(list, remaining_);
  } else {
    XdrList<entry3>* entry3List = &std::get<XdrList<entry3>>(list_);
    return fn(entry3List, remaining_);
  }
}

} // namespace facebook::eden

#endif
