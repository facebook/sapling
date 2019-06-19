/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <optional>
#include "eden/fs/fuse/InodeNumber.h"
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
 * The data is stored in a memory-mapped file and flushed to disk on
 * occasion.  Durability on kernel or disk shutdown is not a primary
 * goal - while the data should be persisted if the process segfaults,
 * InodeTable does not attempt to guarantee all changes were flushed
 * in the case of kernel or disk shutdown. Timestamps and permission
 * bits are easy enough to fix and uncommitted changes are
 * short-lived, and the kernel will flush dirty pages if the process
 * is killed.
 *
 * The storage remains dense - rather than using a free list, upon removal of an
 * entry, the last entry is moved to the removed entry's index.
 *
 * The locking strategy is as follows:
 *
 * The index from inode number to record index is wrapped in a SharedMutex.
 * Most accesses will only take a reader lock unless a new entry is added or
 * an inode number is removed.
 *
 * The contents of each record itself is protected by the FileInode and
 * TreeInode's locks.
 *
 * (Someday it might be worthwhile to investigate whether a freelist is
 * beneficial. If records have stable locations within the file and the file
 * is mapped in chunks, allocated records will have stable pointers, avoiding
 * the need for metadata reads and writes to acquire a lock on the index data
 * structure, at the cost of a guaranteed-dense map.)
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
    return modifyOrInsert<Record>(
        ino,
        [&](auto& existing) { return existing; },
        [&] { return record; },
        [&](auto& existing) { return existing; });
  }

  /**
   * If no value is stored for this inode, calls a function to populate its
   * initial data.  This is more efficient than setDefault when computing the
   * default value is nontrivial.
   *
   * populate is called outside of any InodeTable locks. It's safe for
   * it to be an expensive operation. However, in the case that
   * populateIfNotSet races with another function that inserts a
   * record for this inode, it's possible for populate() to be called
   * but its result not used.
   */
  template <typename PopFn>
  void populateIfNotSet(InodeNumber ino, PopFn&& populate) {
    modifyOrInsert<void>(ino, [&](auto&) {}, populate, [&](auto&) {});
  }

  /**
   * Assign or overwrite a value for this inode.
   */
  void set(InodeNumber ino, const Record& record) {
    modifyOrInsert<void>(
        ino,
        [&](auto& existing) { existing = record; },
        [&] { return record; },
        [&](auto&) {});
  }

  /**
   * If a value is present for the given inode, returns it.  Otherwise, throws
   * std::out_of_range.
   */
  Record getOrThrow(InodeNumber ino) {
    if (auto rv = getOptional(ino)) {
      return *rv;
    } else {
      throw std::out_of_range(
          folly::to<std::string>("no entry in InodeTable for inode ", ino));
    }
  }

  /**
   * If the table has an entry for this inode, returns it.  Otherwise, returns
   * std::nullopt.
   */
  std::optional<Record> getOptional(InodeNumber ino) {
    return state_.withRLock([&](const auto& state) -> std::optional<Record> {
      auto iter = state.indices.find(ino);
      if (iter == state.indices.end()) {
        return std::nullopt;
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
    return state_.withRLock([&](auto& state) {
      auto iter = state.indices.find(ino);
      if (iter == state.indices.end()) {
        throw std::out_of_range(
            folly::to<std::string>("no entry in InodeTable for inode ", ino));
      }
      auto index = iter->second;
      CHECK_LT(index, state.storage.size());
      fn(state.storage[index].record);
      // TODO: maybe trigger a background msync
      return state.storage[index].record;
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
        // While transitioning metadata from the overlay to the
        // InodeMetadataTable, it is common for there to be no metadata for an
        // inode whose number is known. The Overlay calls freeInode()
        // unconditionally, so simply do nothing.
        return;
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

  /**
   * Iterate over all entries of the table and call fn with the inode
   * and record
   *
   * `fn` has type (const InodeNumber&, Record&) -> void
   */
  template <typename ModifyFn>
  void forEachModify(ModifyFn&& fn) {
    auto state = state_.wlock();
    for (auto& entry : state->indices) {
      const auto& inode = entry.first;
      auto index = entry.second;
      auto& record = state->storage[index].record;
      fn(inode, record);
    }
  }

 private:
  explicit InodeTable(MappedDiskVector<Entry>&& storage)
      : state_{folly::in_place, std::move(storage)} {}

  /**
   * Helper function that, in the common case that this inode number
   * already has an entry, only acquires an rlock. If it does not
   * exist, then a write lock is acquired and a new entry is inserted.
   *
   * In the common case, the only invoked callback is `modify`. If an
   * entry does not exist, `create` is called prior to acquiring the
   * write lock. If an entry has been inserted in the meantime, the
   * result of `create` is discarded and `modify` is called
   * instead. If we did use the result of `create`, modifyOrInsert returns
   * the result of `result` applied to the newly-inserted record.
   *
   * `modify` has type Record& -> T
   * `create` has type () -> Record
   * `result` has type Record& -> T
   *
   * WARNING: `modify` and `result` are called while the state lock is
   * held. `create` is called while no locks are held.
   */
  template <typename T, typename ModifyFn, typename CreateFn, typename ResultFn>
  T modifyOrInsert(
      InodeNumber ino,
      ModifyFn&& modify,
      CreateFn&& create,
      ResultFn&& result) {
    // First, acquire the rlock. If an entry exists for `ino`, we can call
    // modify immediately.
    {
      auto state = state_.rlock();
      auto iter = state->indices.find(ino);
      if (LIKELY(iter != state->indices.end())) {
        auto index = iter->second;
        return modify(state->storage[index].record);
      }
    }

    // Construct the new Record while no lock is held in case it does anything
    // expensive.
    Record record = create();

    auto state = state_.wlock();
    // Check again - something may have raced between the locks.
    auto iter = state->indices.find(ino);
    if (UNLIKELY(iter != state->indices.end())) {
      auto index = iter->second;
      return modify(state->storage[index].record);
    }

    size_t index = state->storage.size();
    state->storage.emplace_back(ino, record);
    state->indices.emplace(ino, index);
    return result(state->storage[index].record);
  }

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
     * Holds the actual records, indexed by the values in indices_. The
     * records are stored densely. Freeing an inode moves the last entry into
     * the newly-freed hole.
     *
     * Mutable because we want the ability to modify entries of the vector
     * (but not change its size) while only the index's rlock is held. That is,
     * multiple inodes should be able to update their metadata at the same time.
     */
    mutable MappedDiskVector<Entry> storage;

    /// Maintains an index from inode number to index in storage_.
    std::unordered_map<InodeNumber, size_t> indices;
  };

  folly::Synchronized<State> state_;
}; // namespace eden

static_assert(
    sizeof(InodeMetadata) == 40,
    "Don't change InodeMetadata without implementing a migration path");

using InodeMetadataTable = InodeTable<InodeMetadata>;

} // namespace eden
} // namespace facebook
