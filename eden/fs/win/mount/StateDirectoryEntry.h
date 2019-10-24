/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <map>
#include "eden/fs/model/Hash.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/RegUtils.h"
#include "eden/fs/win/utils/StringConv.h"

namespace facebook {
namespace eden {
/**
 * EntryState contains the different states to represent the current state of a
 * directory entry in projfs namespace.
 */
enum class EntryState {
  /**
   * None is mostly invalid entry and represents something which got created as
   * a part of creating a path.
   */
  NONE = 0,

  /**
   * A Created entry is the one which is backed by backing store. The Prjfs only
   * has the metadata for this file and no data. The directories will also be in
   * this state when first created.
   */
  CREATED = 1,

  /**
   * A file will be in loaded state when the first read or write operation is
   * executed on it. This state means that Prjfs has data for the file. This
   * state is invalid for the directories.
   */
  LOADED = 2,

  /**
   * An entry will be marked MATERIALIZED when it's not back by the source
   * control. A modified or a newly created file will be materialized. It is
   * valid for a directory to be in this state.
   */
  MATERIALIZED = 3,

  /**
   * A file or directory will be in removed state if it was deleted. If a
   * directory is deleted the regdb should contain all the source control
   * entries for the sub-entries in removed state.
   */
  REMOVED = 4
};

/**
 * StateInfo is a bit structure for flags. The first 4 bits store the
 * EntryState. The reserved fields are unused and can be used in future.
 */

struct StateInfo {
  /**
   * The whole nibble is used to store entry state so it is easier to figure
   * out the state of a directory entry while debugging in the hex
   * representation.
   */
  EntryState entryState : 4;

  /**
   * These are unused flags and could be used in future. One reason to have
   * a nibble size unused flag in between is so it is easier to spot the
   * EntryState in memory.
   */
  uint32_t unused1 : 4;

  /**
   * isDirectory flag is used for to differentiate between the file and
   * directory.
   */
  uint32_t isDirectory : 1;

  /**
   * When hasHash is set means we the entry is backed by the source control.
   * When hasHash is 0, this also means that the entry should be in
   * MATERIALIZED EntryState.
   */
  uint32_t hasHash : 1;

  /**
   * wasDeleted is for the entries which were deleted in the past and then
   * recreated.
   */
  uint32_t wasDeleted : 1;
  uint32_t unused2 : 21;

  StateInfo() {
    std::memset(this, 0, sizeof(StateInfo));
  }

  StateInfo(EntryState st, bool isDirectory, bool hasHash) {
    std::memset(this, 0, sizeof(StateInfo));
    entryState = st;
    this->isDirectory = isDirectory ? 1 : 0;
    this->hasHash = hasHash ? 1 : 0;
  }

  StateInfo(DWORD st) {
    std::memcpy(this, &st, sizeof(DWORD));
  }

  DWORD toDWord() const {
    DWORD st;
    std::memcpy(&st, this, sizeof(DWORD));
    return st;
  }

  bool operator==(const StateInfo& other) const {
    return (std::memcmp(this, &other, sizeof(StateInfo)) == 0);
  }

  bool operator!=(const StateInfo& other) const {
    return (std::memcmp(this, &other, sizeof(StateInfo)) != 0);
  }
};

/**
 * In this static assert, we check against DWORD, which is the actual size of
 * our storage in the regdb.
 */
static_assert(sizeof(StateInfo) == sizeof(DWORD));

static inline const char* entryStateCodeToString(EntryState state) {
  switch (state) {
    case EntryState::CREATED:
      return "CREATED";

    case EntryState::LOADED:
      return "LOADED";

    case EntryState::MATERIALIZED:
      return "MATERIALIZED";

    case EntryState::REMOVED:
      return "REMOVED";

    default:
      return "Unknown";
  }
}

/**
 * The StateDirectoryEntry is an in-memory representation of a directory entry
 * state in the Prjfs cache. This structure will be returned by the query
 * operations on the regdb and will encapsulate the info about directory entry.
 */
class StateDirectoryEntry {
 public:
  StateDirectoryEntry(
      std::shared_ptr<WinRelativePathW> parent,
      std::wstring name,
      StateInfo info,
      const Hash& hash)
      : parent_{std::move(parent)},
        name_{std::move(name)},
        info_{info},
        scmHash_{hash} {
    DCHECK(info_.hasHash == 1);
  }
  StateDirectoryEntry(
      std::shared_ptr<WinRelativePathW> parent,
      std::wstring name,
      StateInfo info)
      : parent_{std::move(parent)}, name_{std::move(name)}, info_{info} {
    DCHECK(info_.hasHash == 0);
  }

  ~StateDirectoryEntry() = default;

  [[nodiscard]] bool isDirectory() const {
    return info_.isDirectory;
  }

  [[nodiscard]] bool hasHash() const {
    return info_.hasHash;
  }

  [[nodiscard]] EntryState state() const {
    return info_.entryState;
  }

  [[nodiscard]] Hash getHash() const {
    return scmHash_;
  }

  [[nodiscard]] bool wasDeleted() const {
    return (info_.wasDeleted != 0);
  }

  [[nodiscard]] const WinPathComponentW& getName() const {
    return name_;
  }

  [[nodiscard]] const WinRelativePathW& getParentPath() const {
    return *(parent_.get());
  }

  // No copy and move because I don't think we will need it. If we do we will
  // update this code.
  StateDirectoryEntry(const StateDirectoryEntry&) = delete;
  StateDirectoryEntry& operator=(const StateDirectoryEntry&) = delete;

  StateDirectoryEntry(StateDirectoryEntry&& other)
      : parent_{std::move(other.parent_)},
        name_{std::move(other.name_)},
        info_{other.info_},
        scmHash_{other.scmHash_} {}

  StateDirectoryEntry& operator=(StateDirectoryEntry&& other) {
    if (this != &other) {
      info_ = other.info_;
      scmHash_ = other.scmHash_;
      name_ = std::move(other.name_);
      parent_ = std::move(other.parent_);
    }
    return *this;
  }

  bool operator==(const StateDirectoryEntry& other) const {
    if ((info_ == other.info_) && (scmHash_ == other.scmHash_) &&
        (name_ == other.name_) && (parent_ == other.parent_)) {
      return true;
    }
    return false;
  }

  bool operator!=(const StateDirectoryEntry& other) const {
    return !(*this == other);
  }

 private:
  std::shared_ptr<WinRelativePathW> parent_;
  WinPathComponentW name_;
  StateInfo info_;
  Hash scmHash_;
};

} // namespace eden
} // namespace facebook
