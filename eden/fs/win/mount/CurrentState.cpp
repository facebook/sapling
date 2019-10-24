/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include "CurrentState.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/win/mount/StateDbNode.h"
#include "eden/fs/win/store/WinStore.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/logging/xlog.h"

namespace facebook {
namespace eden {

#ifdef NDEBUG
#define LOG_STATE_CHANGE(fmt, ...)
#else
#define LOG_STATE_CHANGE(fmt, ...) XLOGF(INFO, fmt, ##__VA_ARGS__)
#endif

//
// In the current design we could get away without any locking in this for two
// reasons. First, all FS notifications are synchronous so file system will not
// send us multiple notifications for the same file. Plus registry has its own
// internal locking to protect its structure.
//
// Based on how it performs we might want to serialize the request in here.
// Which could take care of both performance and atomicity.
//

void CurrentState::entryCreated(
    ConstWinRelativePathWPtr path,
    const FileMetadata& metadata) {
  DCHECK(std::filesystem::path(path).filename() == metadata.name);

  DWORD disposition;
  StateDbNode dbNode{path, rootKey_.create(path, KEY_ALL_ACCESS, &disposition)};
  // Either it's a new key or the state was deleted
  DCHECK(
      (disposition == REG_CREATED_NEW_KEY) ||
      (dbNode.getEntryState() == EntryState::REMOVED));

  if ((dbNode.getEntryState() != EntryState::REMOVED)) {
    //
    // Sometimes Prjfs calls getFileInfo to fetch the file details even when it
    // is deleted. We have seen mostly in rename calls where the deleted file is
    // a dest. Not updating our structures in that case.
    //
    LOG_STATE_CHANGE("{} NONE -> CREATED", winToEdenPath(path));
    dbNode.setEntryState(EntryState::CREATED);
    dbNode.setIsDirectory(metadata.isDirectory);
    dbNode.setHash(metadata.hash);
  }
}

void CurrentState::entryLoaded(ConstWinRelativePathWPtr path) {
  StateDbNode dbNode{path, rootKey_.openSubKey(path)};
  DCHECK(dbNode.isDirectory() == false);

  LOG_STATE_CHANGE(
      "{} {} -> LOADED",
      winToEdenPath(path),
      entryStateCodeToString(dbNode.getEntryState()));

  dbNode.setEntryState(EntryState::LOADED);
}

void CurrentState::fileCreated(
    ConstWinRelativePathWPtr path,
    bool isDirectory) {
  DWORD disposition;
  StateDbNode dbNode{path, rootKey_.create(path, KEY_ALL_ACCESS, &disposition)};

  // Either it's a new key or the state was deleted
  DCHECK(
      (disposition == REG_CREATED_NEW_KEY) ||
      (dbNode.getEntryState() == EntryState::REMOVED));

  LOG_STATE_CHANGE("{} NONE -> MATERIALIZED", winToEdenPath(path));

  dbNode.setEntryState(EntryState::MATERIALIZED);
  dbNode.setIsDirectory(isDirectory);
  dbNode.resetHash();
}

void CurrentState::fileModified(
    ConstWinRelativePathWPtr path,
    bool isDirectory) {
  StateDbNode dbNode{path, rootKey_.openSubKey(path)};

  DCHECK_EQ(dbNode.isDirectory(), isDirectory);

  LOG_STATE_CHANGE(
      "{} {} -> MATERIALIZED",
      winToEdenPath(path),
      entryStateCodeToString(dbNode.getEntryState()));

  dbNode.setEntryState(EntryState::MATERIALIZED);
}

void CurrentState::fileRenamed(
    ConstWinRelativePathWPtr oldPath,
    ConstWinRelativePathWPtr newPath,
    bool isDirectory) {
  if (oldPath != nullptr) {
    fileRemoved(oldPath, isDirectory);
  }
  return fileCreated(newPath, isDirectory);
}

void CurrentState::fileRemoved(
    ConstWinRelativePathWPtr path,
    bool isDirectory) {
  StateDbNode dbNode{path, rootKey_.openSubKey(path)};

  LOG_STATE_CHANGE(
      "{} {} -> REMOVED",
      winToEdenPath(path),
      entryStateCodeToString(dbNode.getEntryState()));

  dbNode.setEntryState(EntryState::REMOVED);
}

} // namespace eden
} // namespace facebook
