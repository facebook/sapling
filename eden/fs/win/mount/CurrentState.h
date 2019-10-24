/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/win/mount/StateDbNode.h"
#include "eden/fs/win/utils/RegUtils.h"
#include "eden/fs/win/utils/StringConv.h"

namespace facebook {
namespace eden {

/**
 * CurrentState is top level interface for recording the notifications to
 * replicate the cache state in the internal db in usermode.
 */
class CurrentState {
 public:
  /**
   * root is the regdb root path to the eden current state data.
   *
   * mountId is a unique identifier for this mount. This need to be same across
   * restarts.
   */
  explicit CurrentState(
      const std::wstring_view& root,
      const std::wstring& mountId)
      : path_{WinRelativePathW(root) / mountId},
        rootKey_{RegistryKey::create(HKEY_CURRENT_USER, path_.c_str())} {}

  /**
   * entryCreated is to record the Prjfs's meatadata request. This takes path of
   * the file or directory and the metadata information.
   */
  void entryCreated(
      ConstWinRelativePathWPtr path,
      const FileMetadata& metadata);

  /**
   * entryLoaded is to record the Prjfs's file data request. This request is not
   * valid for the directories.
   */
  void entryLoaded(ConstWinRelativePathWPtr path);

  /**
   * fileCreated records the creation of a new file. These are newly created
   * files which aren't backed by backing store.
   */
  void fileCreated(ConstWinRelativePathWPtr path, bool isDirectory);

  /**
   * fileModified - records the modification of a newly created or a backing
   * store backed file.
   */
  void fileModified(ConstWinRelativePathWPtr path, bool isDirectory);

  /**
   * fileRenamed is to record the rename of a file or directory.
   */
  void fileRenamed(
      ConstWinRelativePathWPtr oldPath,
      ConstWinRelativePathWPtr newPath,
      bool isDirectory);

  /**
   * fileRemoved is to record the deletion of a file and directory.
   */

  void fileRemoved(ConstWinRelativePathWPtr path, bool isDirectory);

  /**
   * API to get the StateDbNode for the relative path of a file.
   */
  [[nodiscard]] StateDbNode getDbNode(const WinRelativePathW& path) const {
    return StateDbNode{path, rootKey_.openSubKey(path.c_str())};
  }

 private:
  /**
   * path_ is the registry path to record Eden repo data.
   */
  const WinRelativePathW path_;

  /**
   * This is the registry key object for the quick access to the data.
   */
  RegistryKey rootKey_;
};

} // namespace eden
} // namespace facebook
