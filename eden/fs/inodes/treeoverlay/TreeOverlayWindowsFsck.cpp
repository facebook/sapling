/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlayWindowsFsck.h"

#ifdef _WIN32
#include <boost/filesystem.hpp>
#include <folly/portability/Windows.h>

#include <ProjectedFSLib.h> // @manual
#include <winioctl.h> // @manual

#include "eden/common/utils/WinError.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/inodes/treeoverlay/TreeOverlay.h"
#include "eden/fs/utils/DirType.h"

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

namespace {
// Reparse tag for UNIX domain socket is not defined in Windows header files.
const ULONG IO_REPARSE_TAG_SOCKET = 0x80000023;

// This is only defined in Windows Device Driver Kit and it is inconvenient to
// include. This is copied from Watchman's FileDescriptor.cpp.
struct REPARSE_DATA_BUFFER {
  ULONG ReparseTag;
  USHORT ReparseDataLength;
  USHORT Reserved;
  union {
    struct {
      USHORT SubstituteNameOffset;
      USHORT SubstituteNameLength;
      USHORT PrintNameOffset;
      USHORT PrintNameLength;
      ULONG Flags;
      WCHAR PathBuffer[1];
    } SymbolicLinkReparseBuffer;
    struct {
      USHORT SubstituteNameOffset;
      USHORT SubstituteNameLength;
      USHORT PrintNameOffset;
      USHORT PrintNameLength;
      WCHAR PathBuffer[1];
    } MountPointReparseBuffer;
    struct {
      UCHAR DataBuffer[1];
    } GenericReparseBuffer;
  };
};
} // namespace

dtype_t dtypeFromEntry(const boost::filesystem::directory_entry& entry) {
  XLOGF(DBG9, "dtypeFromEntry: {}", entry.path().string().c_str());
  auto path = entry.path().wstring();
  WIN32_FILE_ATTRIBUTE_DATA attrs;

  if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &attrs)) {
    XLOGF(
        DBG3,
        "Unable to get file attributes for {}: {}",
        entry.path().string(),
        GetLastError());
    return dtype_t::Unknown;
  }

  if (attrs.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) {
    auto handle = CreateFileW(
        path.c_str(),
        FILE_GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        NULL);
    if (handle == INVALID_HANDLE_VALUE) {
      XLOGF(
          DBG3,
          "Unable to determine reparse point type for {}: {}",
          entry.path().string(),
          GetLastError());
      return dtype_t::Unknown;
    }

    char buffer[MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    DWORD bytes_written;

    if (!DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            NULL,
            0,
            buffer,
            MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
            &bytes_written,
            NULL)) {
      XLOGF(
          DBG3,
          "Unable to read reparse point data for {}: {}",
          entry.path().string(),
          GetLastError());
      return dtype_t::Unknown;
    }

    auto reparse_data = reinterpret_cast<const REPARSE_DATA_BUFFER*>(buffer);

    if (reparse_data->ReparseTag == IO_REPARSE_TAG_SYMLINK) {
      return dtype_t::Symlink;
    } else if (reparse_data->ReparseTag == IO_REPARSE_TAG_SOCKET) {
      return dtype_t::Regular;
    }

    // We don't care about other reparse point types, so treating them as
    // regular files.
    return dtype_t::Regular;
  } else if (attrs.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
    return dtype_t::Dir;
  } else {
    return dtype_t::Regular;
  }
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
    bool recordDeletion,
    TreeOverlay::LookupCallback& callback) {
  auto boostPath = boost::filesystem::path(dir.stringPiece());
  if (!boost::filesystem::is_directory(boostPath)) {
    XLOGF(WARN, "Attempting to scan '{}' which is not a directory", dir);
    return;
  }

  XLOGF(DBG3, "Scanning {}", dir);

  auto overlayEntries = makeEntriesSet(knownState);
  // Loop to synchronize overlay state with disk state
  for (const auto& entry : boost::filesystem::directory_iterator(boostPath)) {
    auto path = AbsolutePath{entry.path().c_str()};
    auto name = path.basename();
    auto dtype = dtypeFromEntry(entry);

    // TODO: EdenFS for Windows does not support symlinks yet, the only
    // symlink we have are redirection points.
    if (dtype == dtype_t::Symlink) {
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

    if (presentInOverlay) {
      auto overlayEntry = getEntryFromOverlayDir(knownState, name);
      // TODO: remove cast once we don't use Thrift to represent overlay entry
      auto overlayDtype =
          mode_to_dtype(static_cast<mode_t>(*overlayEntry->mode_ref()));

      // Check if the user has created a different kind of file with the same
      // name. For example, overlay thinks one file is a file while it's now a
      // directory on disk.
      if (overlayDtype != dtype) {
        XLOGF(
            DBG3,
            "Mismatch file type, expected: {} overlay: {}",
            dtype,
            overlayDtype);
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
      overlayEntry.set_mode(dtype_to_mode(dtype));
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
  for (const auto& entry : boost::filesystem::directory_iterator(boostPath)) {
    auto path = AbsolutePath{entry.path().c_str()};
    auto mode = dtypeFromEntry(entry);

    // We can't scan non-directories nor follow symlinks
    if (mode == dtype_t::Symlink) {
      XLOGF(DBG5, "Skipped {} since it's a symlink", path);
      continue;
    } else if (mode != dtype_t::Dir) {
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
      scanCurrentDir(overlay, path, entryInode, *entryDir, isFull, callback);
    }
  }
}
} // namespace

void windowsFsckScanLocalChanges(
    FOLLY_MAYBE_UNUSED std::shared_ptr<const EdenConfig> config,
    TreeOverlay& overlay,
    AbsolutePathPiece mountPath,
    TreeOverlay::LookupCallback& callback) {
  XLOGF(INFO, "Start scanning {}", mountPath);
  if (auto view = overlay.loadOverlayDir(kRootNodeId)) {
    scanCurrentDir(overlay, mountPath, kRootNodeId, *view, false, callback);
    XLOGF(INFO, "Scanning complete for {}", mountPath);
  } else {
    XLOG(INFO)
        << "Unable to start fsck since root inode is not present. Possibly new mount.";
  }
}

} // namespace facebook::eden

#endif
