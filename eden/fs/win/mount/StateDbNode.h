/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/model/Hash.h"
#include "eden/fs/win/mount/StateDirectoryEntry.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/RegUtils.h"
#include "eden/fs/win/utils/StringConv.h"

namespace facebook {
namespace eden {

/**
 * StateDbNode is an interface to fetch and set the StateDirectoryEntry to and
 * from RegDb. This class is not multi-thread safe and the call needs to be
 * synchronized by the caller.
 *
 * TODO(puneetk): We need to add interfaces which could fetch and set all the
 * entries in a single call.
 */
class StateDbNode {
 public:
  /**
   * path is relative path to the file or directory we need to get or set info
   * on.
   *
   * key is the Regdb entry which will for the relative path.
   */
  explicit StateDbNode(const WinRelativePathW& path, RegistryKey&& key)
      : path_{std::make_shared<WinRelativePathW>(path)},
        tree_{std::move(key)} {}

  StateDbNode(const StateDbNode&) = delete;
  StateDbNode& operator=(const StateDbNode&) = delete;

  StateDbNode(StateDbNode&& other) noexcept
      : path_{std::move(other.path_)}, tree_{std::move(other.tree_)} {}

  StateDbNode& operator=(StateDbNode&& other) noexcept {
    if (this != &other) {
      tree_ = std::move(other.tree_);
      path_ = std::move(other.path_);
    }
    return *this;
  }

  ~StateDbNode() = default;

  /**
   * getStateInfo returns the StateInfo from the Regdb entry. If the entry is
   * not found it will return the StateInfo{0}. It will throw on all other
   * errors.
   */
  StateInfo getStateInfo(RegistryPath subKey = nullptr) const {
    try {
      return tree_.getDWord(kStateInfoValue, subKey);
    } catch (const std::system_error& ex) {
      // Fetching the status info before setting it could result in not found.
      // Return 0 for not found - rethrow all other exceptions.
      if (ex.code().value() == ERROR_FILE_NOT_FOUND) {
        return 0;
      }
      throw;
    }
  }

  /**
   * Set the StateInfo. StateInfo is DWORD long and should fit in a DWORD entry
   * on the Regdb.
   */
  void setStateInfo(StateInfo info) {
    tree_.setDWord(kStateInfoValue, info.toDWord());
  }

  /**
   * getDirectoryEntries will return a vector of all the state dir entries.
   */
  [[nodiscard]] std::vector<StateDirectoryEntry> getDirectoryEntries() const {
    auto entries = tree_.enumerateKeys();
    std::vector<StateDirectoryEntry> dirEntries;

    for (const auto& entry : entries) {
      auto info = getStateInfo(entry.c_str());
      const bool hasHash = (info.hasHash != 0);

      if (hasHash) {
        Hash::Storage hashBuffer;
        tree_.getBinary(
            kHashValue, hashBuffer.data(), hashBuffer.size(), entry.c_str());
        dirEntries.emplace_back(path_, entry, info, Hash{hashBuffer});
      } else {
        dirEntries.emplace_back(path_, entry, info);
      }
    }
    return dirEntries;
  }

  [[nodiscard]] Hash getHash() const {
    Hash::Storage hashBuffer;
    tree_.getBinary(kHashValue, hashBuffer.data(), hashBuffer.size(), nullptr);
    return Hash{hashBuffer};
  }

  [[nodiscard]] bool isDirectory() const {
    return (getStateInfo().isDirectory != 0);
  }

  [[nodiscard]] bool hasHash() const {
    return (getStateInfo().hasHash != 0);
  }

  [[nodiscard]] EntryState getEntryState() const {
    return (getStateInfo().entryState);
  }

  void setHash(Hash hash) {
    const auto hashBuffer = hash.getBytes();
    tree_.setBinary(kHashValue, hashBuffer.data(), hashBuffer.size());
    auto info = getStateInfo();
    info.hasHash = 1;
    setStateInfo(info);
  }

  void resetHash() {
    auto info = getStateInfo();
    info.hasHash = 0;
    setStateInfo(info);
  }

  void setIsDirectory(bool isDirectory) {
    auto info = getStateInfo();
    info.isDirectory = isDirectory ? 1 : 0;
    setStateInfo(info);
  }

  void setEntryState(EntryState state) {
    auto info = getStateInfo();
    if (state == EntryState::REMOVED) {
      info.wasDeleted = 1;
    }
    info.entryState = state;
    setStateInfo(info);
  }

 private:
  /**
   * path_ contains relative path from the root of the mount. It would be
   * shared between this and the StateDirectoryEntries.
   */
  std::shared_ptr<WinRelativePathW> path_;
  RegistryKey tree_;

  /**
   * Create these constants as wstring instead of piece because they need to be
   * null-terminated. This will save us to create wstring on every call.
   */
  static const std::wstring kHashValue;
  static const std::wstring kStateInfoValue;
};
} // namespace eden
} // namespace facebook
