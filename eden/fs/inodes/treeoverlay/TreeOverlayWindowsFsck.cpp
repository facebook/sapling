/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlayWindowsFsck.h"

#ifdef _WIN32
#include <boost/filesystem.hpp>
#include <folly/portability/Windows.h>

#include <ProjectedFSLib.h> // @manual

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/WinError.h"

namespace facebook::eden {
namespace {

namespace boost_fs = boost::filesystem;

PRJ_FILE_STATE getPrjFileState(AbsolutePathPiece entry) {
  auto wpath = entry.wide();
  PRJ_FILE_STATE state;

  auto result = PrjGetOnDiskFileState(wpath.c_str(), &state);
  if (FAILED(result)) {
    throwHResultErrorExplicit(result, "Unable to get ProjectedFS file state");
  }

  return state;
}

// Generate a set of filenames from a given overlay directory.
std::set<PathComponent> makeEntriesSet(const overlay::OverlayDir& dir) {
  std::set<PathComponent> result;
  const auto& entries = dir.entries_ref();
  for (auto entry = entries->cbegin(); entry != entries->cend(); entry++) {
    result.emplace(entry->first);
  }
  return result;
}

mode_t modeFromEntry(const boost::filesystem::directory_entry& entry) {
  // NOTE: Both boost::filesystem and MSVC's std::filesystem only supports
  // detecting regular file, directory and symlink. This should be sufficient
  // for us.
  if (boost_fs::is_regular_file(entry)) {
    return dtype_to_mode(dtype_t::Regular);
  } else if (boost_fs::is_directory(entry)) {
    return dtype_to_mode(dtype_t::Dir);
  } else if (boost_fs::is_symlink(entry)) {
    return dtype_to_mode(dtype_t::Symlink);
  }
  XLOGF(
      DBG5,
      "Failed to get file mode for file: {}, status is: {}",
      entry.path().string(),
      entry.status().type());
  return dtype_to_mode(dtype_t::Unknown);
}

std::optional<overlay::OverlayEntry> getEntryFromOverlayDir(
    const overlay::OverlayDir& dir,
    PathComponentPiece name) {
  const auto& entries = *dir.entries_ref();

  // Case insensitive look up
  for (auto iter = entries.begin(); iter != entries.end(); ++iter) {
    if (name.stringPiece().equals(iter->first, folly::AsciiCaseInsensitive())) {
      return std::make_optional<overlay::OverlayEntry>(iter->second);
    }
  }
  return std::nullopt;
}

void removeChildRecursively(TreeOverlay& overlay, InodeNumber inode) {
  XLOGF(DBG9, "Removing directory inode = {} ", inode);
  if (auto dir = overlay.loadOverlayDir(inode)) {
    const auto& entries = dir->entries_ref();
    for (auto iter = entries->cbegin(); iter != entries->cend(); iter++) {
      const auto& entry = iter->second;
      if (S_ISDIR(*entry.mode_ref())) {
        auto entryInode = InodeNumber::fromThrift(*entry.inodeNumber_ref());
        removeChildRecursively(overlay, entryInode);
      }
      XLOGF(DBG9, "Removing child path = {}", iter->first);
      overlay.removeChild(inode, PathComponentPiece{iter->first});
    }
  }
}

// Remove entry from overlay, but recursively if the entry is a directory. This
// is different from `overlay.removeChild` as it does not remove directory
// recursively.
void removeOverlayEntry(
    TreeOverlay& overlay,
    InodeNumber parent,
    PathComponentPiece name,
    std::optional<overlay::OverlayEntry> entry = std::nullopt) {
  if (!entry) {
    auto dir = overlay.loadOverlayDir(parent);
    entry = getEntryFromOverlayDir(*dir, name);
  }
  auto overlayMode = static_cast<mode_t>(*entry->mode_ref());
  if (S_ISDIR(overlayMode)) {
    auto overlayInode = InodeNumber::fromThrift(*entry->inodeNumber_ref());
    removeChildRecursively(overlay, overlayInode);
  }
  overlay.removeChild(parent, name);
}

void scanCurrentDir(
    TreeOverlay& overlay,
    AbsolutePathPiece dir,
    InodeNumber inode,
    overlay::OverlayDir knownState,
    bool recordDeletion) {
  if (!dir.is_directory()) {
    XLOGF(WARN, "Attempting to scan '{}' which is not a directory", dir);
    return;
  }

  XLOGF(DBG3, "Scanning {}", dir);

  auto overlayEntries = makeEntriesSet(knownState);
  // Loop to synchronize overlay state with disk state
  for (const auto& entry :
       boost::filesystem::directory_iterator(dir.as_boost())) {
    auto path = AbsolutePath{entry.path().c_str()};
    auto name = path.basename();

    // TODO: EdenFS for Windows does not support symlinks yet, the only
    // symlink we have are redirection points.
    if (boost_fs::is_symlink(entry)) {
      continue;
    }

    // Check if this entry present in overlay
    bool presentInOverlay = false;
    for (auto iter = overlayEntries.begin(); iter != overlayEntries.end();
         ++iter) {
      if (name.stringPiece().equals(
              iter->stringPiece(), folly::AsciiCaseInsensitive())) {
        // Once we found the entry in overlay, we remove it from the overlay,
        // so we know if there are entries missing from disk at the end.
        overlayEntries.erase(iter);
        presentInOverlay = true;
        break;
      }
    }

    auto mode = modeFromEntry(entry);
    if (presentInOverlay) {
      auto overlayEntry = getEntryFromOverlayDir(knownState, name);
      // TODO: remove cast once we don't use Thrift to represent overlay entry
      auto overlayMode = static_cast<mode_t>(*overlayEntry->mode_ref());

      // Check if the user has created a different kind of file with the same
      // name. For example, overlay thinks one file is a file while it's now a
      // directory on disk.
      if (overlayMode != mode) {
        XLOGF(
            DBG3,
            "Mismatch file type, expected: {} overlay: {}",
            mode,
            overlayMode);
        removeOverlayEntry(overlay, inode, name, overlayEntry);
        presentInOverlay = false;
      }
    }

    auto state = getPrjFileState(path);
    auto isTombstone =
        (state & PRJ_FILE_STATE_TOMBSTONE) == PRJ_FILE_STATE_TOMBSTONE;

    // Tombstone residue may still linger around when EdenFS is not running.
    // These represent files are deleted and we should not add them back.
    if (!(presentInOverlay || isTombstone)) {
      // Add current file to overlay
      XLOGF(DBG3, "Adding missing entry to overlay {}", name);
      overlay::OverlayEntry overlayEntry;
      overlayEntry.set_mode(mode);
      overlayEntry.set_inodeNumber(overlay.nextInodeNumber().get());
      overlay.addChild(inode, name, overlayEntry);
    }
  }

  // We can only fully trust the disk state when the directory is Full. A
  // DirtyPlaceholder directory may hide entries that were not previously
  // accessed when EdenFS is not running, which could lead fsck to remove
  // entries from overlay incorrectly.
  if (recordDeletion && !overlayEntries.empty()) {
    // Files in overlay are not present on disk, remove them.
    for (auto removed = overlayEntries.cbegin();
         removed != overlayEntries.cend();
         removed++) {
      XLOGF(DBG3, "Removing missing entry from overlay: {}", *removed);
      removeOverlayEntry(overlay, inode, *removed);
    }
  }

  XLOGF(DBG9, "Reloading {} from overlay.", inode);
  // Reload the updated overlay as we have fixed the inconsistency.
  auto updated = *overlay.loadOverlayDir(inode);

  // Now that this overlay directory is consistent with the on-disk state,
  // proceed to its children.
  for (const auto& entry :
       boost::filesystem::directory_iterator(dir.as_boost())) {
    auto path = AbsolutePath{entry.path().c_str()};
    // We can't scan non-directories nor follow symlinks
    if (!path.is_directory()) {
      continue;
    } else if (boost_fs::is_symlink(entry)) {
      XLOGF(DBG5, "Skipped {} since it's a symlink", path);
      continue;
    }

    auto state = getPrjFileState(path);
    // User can only modify directory content if it is Full or Dirty
    // Placeholder.
    auto isFull = (state & PRJ_FILE_STATE_FULL) == PRJ_FILE_STATE_FULL;
    auto isDirtyPlaceholder = (state & PRJ_FILE_STATE_DIRTY_PLACEHOLDER) ==
        PRJ_FILE_STATE_DIRTY_PLACEHOLDER;
    if (isFull || isDirtyPlaceholder) {
      auto overlayEntry = getEntryFromOverlayDir(updated, path.basename());
      auto entryInode =
          InodeNumber::fromThrift(*overlayEntry->inodeNumber_ref());
      auto entryDir = overlay.loadOverlayDir(entryInode);
      scanCurrentDir(overlay, path, entryInode, *entryDir, isFull);
    }
  }
}
} // namespace

void windowsFsckScanLocalChanges(
    TreeOverlay& overlay,
    AbsolutePathPiece mountPath) {
  XLOG(INFO) << "Start scanning";
  if (auto view = overlay.loadOverlayDir(kRootNodeId)) {
    scanCurrentDir(overlay, mountPath, kRootNodeId, *view, false);
  } else {
    XLOG(INFO)
        << "Unable to start fsck since root inode is not present. Possibly new mount.";
  }
}

} // namespace facebook::eden

#endif
