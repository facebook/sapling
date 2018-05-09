/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/MappedDiskVector.h"

namespace facebook {
namespace eden {

namespace detail {
template <typename Record>
struct InodeTableEntry {
  enum { VERSION = Record::VERSION };

  InodeTableEntry() = delete;
  InodeTableEntry(InodeNumber ino, const Record& rec)
      : inode{ino}, record{rec} {}

  /// Conversion from old versions.
  template <typename OldRecord>
  explicit InodeTableEntry(const InodeTableEntry<OldRecord>& old)
      : inode{old.inode}, record{old.record} {}

  // WARNING: this data structure is serialized directly to disk via
  // MappedDiskVector. Do not change the order, set, or types of fields. We
  // could, if we want to change Entry itself, coopt high bits of VERSION and
  // modify MappedDiskVector to allow direct upgrades rather than linear
  // upgrades.
  InodeNumber inode;
  // TODO: should we maintain a 64-bit SpookyHashV2 checksum to ignore
  // corrupted entries?
  Record record;
};
} // namespace detail

/**
 * InodeTable is an efficient storage engine for fixed-size inode records.
 * It is intended for timestamps and mode bits (and any additional fixed-size
 * per-inode state.)
 *
 * The data is stored in a memory-mapped file and flushed to disk on occasion.
 * Durability on kernel or disk shutdown is not a primary goal. Timestamps and
 * permission bits are easy enough to fix and uncommitted changes are
 * short-lived, and the kernel will flush dirty pages if the process is killed.
 *
 * Rather than using a free list, upon removal of an entry, the last entry is
 * moved to the removed entry's index.
 *
 * The locking strategy is as follows:
 *
 * The index from inode number to record index is wrapped in a SharedMutex.
 * Most accesses will only take a reader lock unless a new entry is added or
 * an inode number is removed.
 *
 * The contents of each record itself is protected by the FileInode and
 * TreeInode's locks.
 */
template <typename Record>
class InodeTable {
 public:
  using Entry = detail::InodeTableEntry<Record>;

  InodeTable() = delete;
  InodeTable(const InodeTable&) = delete;
  InodeTable(InodeTable&&) = delete;

  InodeTable& operator=(const InodeTable&) = delete;
  InodeTable& operator=(InodeTable&&) = delete;

  /**
   * Create or open an InodeTable at the specified path.
   */
  template <typename... OldRecords>
  static std::unique_ptr<InodeTable> open(folly::StringPiece path) {
    return std::unique_ptr<InodeTable>{
        new InodeTable{MappedDiskVector<Entry>::template open<
            detail::InodeTableEntry<OldRecords>...>(path, true)}};
  }

  /**
   * If no value is stored for this inode, assigns one.  Returns the new value,
   * whether it was set to the default or not.
   */
  Record setDefault(InodeNumber ino, const Record& record) {
    return state_.withULockPtr([&](auto&& ulock) {
      const auto& indices = ulock->indices;
      auto iter = indices.find(ino);
      if (iter != indices.end()) {
        return ulock->storage[iter->second].record;
      } else {
        auto wlock = ulock.moveFromUpgradeToWrite();

        size_t index = wlock->storage.size();
        wlock->storage.emplace_back(ino, record);
        wlock->indices.emplace(ino, index);
        return wlock->storage[index].record;
      }
    });
  }

  /**
   * If no value is stored for this inode, calls a function to populate its
   * initial data.  This is more efficient than setDefault when computing the
   * default value is nontrivial.
   *
   * Note that the callback is run while the table's locks are held. Don't
   * call any other InodeTable methods from it.
   */
  template <typename PopFn>
  void populateIfNotSet(InodeNumber ino, PopFn&& pop) {
    return state_.withULockPtr([&](auto&& ulock) {
      const auto& indices = ulock->indices;
      auto iter = indices.find(ino);
      if (iter != indices.end()) {
        return;
      } else {
        auto wlock = ulock.moveFromUpgradeToWrite();

        size_t index = wlock->storage.size();
        wlock->storage.emplace_back(ino, pop());
        wlock->indices.emplace(ino, index);
      }
    });
  }

  /**
   * Assign or overwrite a value for this inode.
   */
  void set(InodeNumber ino, const Record& record) {
    return state_.withWLock([&](auto& state) {
      const auto& indices = state.indices;
      auto iter = indices.find(ino);
      size_t index;
      if (iter != indices.end()) {
        index = iter->second;
        assert(ino == state.storage[index].inode);
        state.storage[index].record = record;
      } else {
        index = state.storage.size();
        state.storage.emplace_back(ino, record);
        state.indices.emplace(ino, index);
      }
    });
  }

  /**
   * If a value is present for the given inode, returns it.  Otherwise, throws
   * std::out_of_range.
   */
  Record getOrThrow(InodeNumber ino) {
    auto rv = getOptional(ino);
    if (rv) {
      return *rv;
    } else {
      throw std::out_of_range(
          folly::to<std::string>("no entry in InodeTable for inode ", ino));
    }
  }

  /**
   * If the table has an entry for this inode, returns it.  Otherwise, returns
   * folly::none.
   */
  folly::Optional<Record> getOptional(InodeNumber ino) {
    return state_.withRLock([&](const auto& state) -> folly::Optional<Record> {
      auto iter = state.indices.find(ino);
      if (iter == state.indices.end()) {
        return folly::none;
      } else {
        auto index = iter->second;
        CHECK_LT(index, state.storage.size());
        return state.storage[index].record;
      }
    });
  }

  /**
   * Calls a function that can modify the data at the given InodeNumber.  Throws
   * std::out_of_range if there is no record.
   *
   * Note that the callback is run while the table's locks are held.  Don't
   * call any other InodeTable methods from it.
   */
  template <typename ModFn>
  Record modifyOrThrow(InodeNumber ino, ModFn&& fn) {
    return state_.withWLock([&](const auto& state) {
      auto iter = state.indices.find(ino);
      if (iter == state.indices.end()) {
        throw std::out_of_range(
            folly::to<std::string>("no entry in InodeTable for inode ", ino));
      }
      auto index = iter->second;
      CHECK_LT(index, state.storage.size());
      fn(state.storage[index]);
      // TODO: maybe trigger a background msync
      return state.storage[index];
    });
  }

  // TODO: replace with freeInodes - it's much more efficient to free a bunch
  // at once.
  void freeInode(InodeNumber ino) {
    state_.withWLock([&](auto& state) {
      auto& storage = state.storage;
      auto& indices = state.indices;

      auto iter = indices.find(ino);
      if (iter == indices.end()) {
        EDEN_BUG() << "tried to deallocate unknown (or already freed) inode";
      }

      size_t indexToDelete = iter->second;
      indices.erase(iter);

      DCHECK_GT(storage.size(), 0);
      size_t lastIndex = storage.size() - 1;

      if (lastIndex != indexToDelete) {
        auto lastInode = storage[lastIndex].inode;
        storage[indexToDelete] = storage[lastIndex];
        indices[lastInode] = indexToDelete;
      }

      storage.pop_back();
    });
  }

 private:
  explicit InodeTable(MappedDiskVector<Entry>&& storage)
      : state_{folly::in_place, std::move(storage)} {}

  struct State {
    State(MappedDiskVector<Entry>&& mdv) : storage{std::move(mdv)} {
      for (size_t i = 0; i < storage.size(); ++i) {
        const Entry& entry = storage[i];
        auto ret = indices.insert({entry.inode, i});
        if (!ret.second) {
          XLOG(WARNING) << "Duplicate records for the same inode: indices "
                        << indices[entry.inode] << " and " << i;
          continue;
        }
      }
    }

    /**
     * Holds the actual records, indexed by the values in indices_.  The
     * records are stored densely.  Freeing an inode moves the last entry into
     * the newly-freed hole.
     */
    MappedDiskVector<Entry> storage;

    /// Maintains an index from inode number to index in storage_.
    std::unordered_map<InodeNumber, size_t> indices;
  };

  folly::Synchronized<State> state_;
};

static_assert(
    sizeof(InodeMetadata) == 40,
    "Don't change InodeMetadata without implementing a migration path");

using InodeMetadataTable = InodeTable<InodeMetadata>;

} // namespace eden
} // namespace facebook
