/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlayWindowsFsck.h"
#include <boost/filesystem/operations.hpp>

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
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/FileUtils.h"

namespace facebook::eden {
namespace {

namespace boost_fs = boost::filesystem;

// TODO
// - test/fix behavior when offline

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

dtype_t dtypeFromAttrs(const wchar_t* path, DWORD dwFileAttributes) {
  if (dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) {
    FileHandle handle{CreateFileW(
        path,
        FILE_GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        NULL)};
    if (handle.get() == INVALID_HANDLE_VALUE) {
      XLOGF(
          DBG3,
          "Unable to determine reparse point type for {}: {}",
          AbsolutePath{path},
          GetLastError());
      return dtype_t::Unknown;
    }

    char buffer[MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    DWORD bytes_written;

    if (!DeviceIoControl(
            handle.get(),
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
          AbsolutePath{path},
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
    // regular files/directories.
    if (dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
      return dtype_t::Dir;
    } else {
      return dtype_t::Regular;
    }
  } else if (dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
    return dtype_t::Dir;
  } else {
    return dtype_t::Regular;
  }
}

dtype_t dtypeFromPath(const boost_fs::path& boostPath) {
  WIN32_FILE_ATTRIBUTE_DATA attrs;
  auto path = boostPath.wstring();

  if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &attrs)) {
    XLOGF(
        DBG3,
        "Unable to get file attributes for {}: {}",
        boostPath.string(),
        GetLastError());
    return dtype_t::Unknown;
  }
  return dtypeFromAttrs(path.c_str(), attrs.dwFileAttributes);
}

PathMap<overlay::OverlayEntry> toPathMap(overlay::OverlayDir& dir) {
  PathMap<overlay::OverlayEntry> newMap(CaseSensitivity::Insensitive);
  const auto& entries = *dir.entries_ref();
  for (auto iter = entries.begin(); iter != entries.end(); ++iter) {
    newMap[PathComponentPiece{iter->first}] = iter->second;
  }
  return newMap;
}

std::optional<overlay::OverlayEntry> getEntryFromOverlayDir(
    const PathMap<overlay::OverlayEntry>& dir,
    PathComponentPiece name) {
  auto result = dir.find(name);
  if (result != dir.end()) {
    return std::make_optional<overlay::OverlayEntry>(result->second);
  } else {
    return std::nullopt;
  }
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
    const overlay::OverlayEntry& entry) {
  XLOGF(DBG9, "Remove overlay entry: {}", name);
  auto overlayMode = static_cast<mode_t>(*entry.mode_ref());
  if (S_ISDIR(overlayMode)) {
    auto overlayInode = InodeNumber::fromThrift(*entry.inodeNumber_ref());
    removeChildRecursively(overlay, overlayInode);
  }
  overlay.removeChild(parent, name);
}

// clang-format off
// T = tombstone
//
// for path in union(onDisk_paths, inOverlay_paths, inScm_paths):
//   disk  overlay  scm   action
//    y       n      n      add to overlay, no scm hash.   (If is_placeholder() error since there's no scm to fill it? We could call PrjDeleteFile on it.)
//    y       y      n      fix overlay mode_t to match disk if necessary. (If is_placeholder(), error since there's no scm to fill it?)
//    y       n      y      add to overlay, use scm hash if placeholder-file or empty-placeholder-directory.
//    y       y      y      fix overlay mode_t to match disk if necessary
//    T       n      *      do nothing
//    T       y      *      drop from overlay, recursively
//    n       y      n      remove from overlay
//    n       y      y      fix overlay mode_t to match scm if necessary.
//    n       n      y      add to overlay, use scm hash
//
// Notes:
// - A directory can be "placeholder" even if one of it's recursive descendants
//   is modified. It is only DirtyPlaceholder if a direct child is modified.
// - Tombstone is only visible when eden is not mounted yet. And (maybe?)
//   appears with a delay after eden closes.
// - I think the overlay will treat HydratedPlaceholder, DirtyPlaceholder, and
//   Full identical. All mean the data is on disk and the overlay entry will be a
//   no-scm-hash entry.
// - Since we'll have the scm hash during fsck, we could also verify the overlay
//   hash is correct.
// clang-format on

struct FsckFileState {
  bool onDisk = false;
  // diskMaterialized is true if:
  //  - a file is full
  //  - a directory is full or a descendant is materialized or tombstoned.
  bool diskMaterialized = false;
  // diskEmptyPlaceholder is true if:
  //  - a file is virtual or a placeholder
  //  - a directory is a placeholder and has no children (placeholder or
  //  otherwise)
  bool diskEmptyPlaceholder = false;
  bool diskTombstone = false;
  dtype_t diskDtype = dtype_t::Unknown;

  bool inOverlay = false;
  dtype_t overlayDtype = dtype_t::Unknown;
  std::optional<ObjectId> overlayHash = std::nullopt;
  std::optional<overlay::OverlayEntry> overlayEntry = std::nullopt;

  bool inScm = false;
  dtype_t scmDtype = dtype_t::Unknown;
  std::optional<ObjectId> scmHash = std::nullopt;

  bool shouldExist = false;
  dtype_t desiredDtype = dtype_t::Unknown;
  std::optional<ObjectId> desiredHash = std::nullopt;
};

bool directoryIsEmpty(const wchar_t* path) {
  WIN32_FIND_DATAW findFileData;
  HANDLE h = FindFirstFileExW(
      path, FindExInfoBasic, &findFileData, FindExSearchNameMatch, nullptr, 0);
  if (h == INVALID_HANDLE_VALUE) {
    throw std::runtime_error(
        fmt::format("unable to check directory - {}", AbsolutePath{path}));
  }

  do {
    if (wcscmp(findFileData.cFileName, L".") == 0 ||
        wcscmp(findFileData.cFileName, L"..") == 0) {
      continue;
    }
    FindClose(h);
    return false;
  } while (FindNextFileW(h, &findFileData) != 0);

  auto error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw std::runtime_error(
        fmt::format("unable to check directory - {}", AbsolutePath{path}));
  }

  FindClose(h);
  return true;
}

void populateDiskState(
    AbsolutePathPiece root,
    RelativePathPiece path,
    FsckFileState& state,
    const WIN32_FIND_DATAW& findFileData) {
  auto absPath = root + path;
  auto wPath = std::wstring{L"\\\\?\\"} + absPath.wide();
  dtype_t dtype = dtypeFromAttrs(wPath.c_str(), findFileData.dwFileAttributes);
  if (dtype != dtype_t::Dir && dtype != dtype_t::Regular) {
    // TODO: What do we do with a symlink, or non-regular file.
    return;
  }

  // Some empirical data on the values of reparse, recall, hidden, and
  // system dwFileAttributes, compared with the tombstone and full
  // getPrjFileState values.
  //
  // https://docs.microsoft.com/en-us/windows/win32/projfs/cache-state
  // https://docs.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
  //
  // clang-format off
  //
  // (reparse, recall, hidden, system) => (tomb,  materialized) dwFileAttributes
  // (false,   false,  false,  false)  => (false, true)  attr=16 (DIRECTORY)
  // (false,   false,  false,  false)  => (false, true)  attr=32 (ARCHIVE)
  // (true,    false,  true,   true)   => (true,  false) attr=1062 (REPARSE_POINT | ARCHIVE | HIDDEN | SYSTEM)
  // (true,    false,  false,  false)  => (false, false) attr=1568 (REPARSE_POINT | SPARSE_FILE | ARCHIVE)
  // (true,    true,   false,  false)  => (false, false) attr=4195344 (RECALL_ON_DATA_ACCESS | REPARSE_POINT | DIRECTORY)
  //
  // clang-format on
  // TODO: try to repro FILE_ATTRIBUTE_RECALL_ON_OPEN using a placeholder
  // directory
  auto reparse = (findFileData.dwFileAttributes &
                  FILE_ATTRIBUTE_REPARSE_POINT) == FILE_ATTRIBUTE_REPARSE_POINT;
  auto recall =
      (findFileData.dwFileAttributes & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) ==
      FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
  auto hidden = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN) ==
      FILE_ATTRIBUTE_HIDDEN;
  auto system = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_SYSTEM) ==
      FILE_ATTRIBUTE_SYSTEM;

  bool detectedTombstone = reparse && !recall && hidden && system;
  bool detectedFull = !reparse && !recall;

  state.onDisk = true;
  state.diskDtype = dtype;
  state.diskTombstone = detectedTombstone;

  // It can also be diskMaterialized if a descendant directory is
  // materialized. But that is checked later when processing the children.
  state.diskMaterialized = detectedFull || detectedTombstone;
  // It's an empty placeholder unless it's materialized or it has children.
  state.diskEmptyPlaceholder = !state.diskMaterialized;

  if (dtype == dtype_t::Dir && !directoryIsEmpty(wPath.c_str())) {
    state.diskEmptyPlaceholder = false;
  }
}

void populateOverlayState(
    FsckFileState& state,
    const overlay::OverlayEntry& overlayEntry) {
  state.inOverlay = true;
  state.overlayDtype = mode_to_dtype(*overlayEntry.mode());
  if (overlayEntry.hash().has_value() && !overlayEntry.hash().value().empty()) {
    auto objId = ObjectId(*overlayEntry.hash());
    state.overlayHash = std::move(objId);
  } else {
    state.overlayHash = std::nullopt;
  }
  state.overlayEntry = overlayEntry;
}

void populateScmState(FsckFileState& state, const TreeEntry& treeEntry) {
  state.scmHash = treeEntry.getHash();
  state.scmDtype = treeEntry.getDtype();
  state.inScm = true;
}

InodeNumber addOrUpdateOverlay(
    TreeOverlay& overlay,
    InodeNumber parentInodeNum,
    PathComponentPiece name,
    dtype_t dtype,
    std::optional<ObjectId> hash,
    const PathMap<overlay::OverlayEntry>& parentInsensitiveOverlayDir) {
  if (overlay.hasChild(parentInodeNum, name)) {
    XLOGF(DBG9, "Updating overlay: {}", name);
    overlay.removeChild(parentInodeNum, name);
  } else {
    XLOGF(DBG9, "Add overlay: {}", name);
  }
  auto overlayEntryOpt =
      getEntryFromOverlayDir(parentInsensitiveOverlayDir, name);
  overlay::OverlayEntry overlayEntry;
  if (overlayEntryOpt.has_value()) {
    // Update the existing entry.
    overlayEntry = *overlayEntryOpt;
  } else {
    // It's a new entry, so give it a new inode number.
    overlayEntry.inodeNumber() = overlay.nextInodeNumber().get();
  }
  if (hash.has_value()) {
    overlayEntry.hash() = hash->asString();
  } else {
    overlayEntry.hash().reset();
  }
  overlayEntry.mode() = dtype_to_mode(dtype);
  overlay.addChild(parentInodeNum, name, overlayEntry);
  return InodeNumber(*overlayEntry.inodeNumber());
}

std::optional<InodeNumber> fixup(
    FsckFileState& state,
    TreeOverlay& overlay,
    RelativePathPiece path,
    InodeNumber parentInodeNum,
    const PathMap<overlay::OverlayEntry>& insensitiveOverlayDir) {
  auto name = path.basename();

  if (!state.onDisk) {
    if (state.inScm) {
      state.desiredDtype = state.scmDtype;
      state.desiredHash = state.scmHash;
      state.shouldExist = true;
    }
  } else if (state.diskTombstone) {
    // state.shouldExist defaults to false
  } else { // if file exists normally on disk
    if (!state.inScm && !state.diskMaterialized) {
      // Stop fixing this up since we can't materialize if it's not in scm.
      // TODO: This is likely caused by EdenFS not having called PrjDeleteFile
      // in a previous checkout operation. We should probably call it here or
      // as a post-PrjfsChannel initialization.

      XLOGF(ERR, "Placeholder present on disk but not in SCM - {}", path);
      return std::nullopt;
    } else {
      state.desiredDtype = state.diskDtype;
      state.desiredHash = state.diskMaterialized ? std::nullopt : state.scmHash;
      state.shouldExist = true;
    }
  }

  XLOGF(
      DBG9,
      "shouldExist={}, onDisk={}, inOverlay={}, inScm={}, tombstone={}, materialized={}",
      state.shouldExist,
      state.onDisk,
      state.inOverlay,
      state.inScm,
      state.diskTombstone,
      state.diskMaterialized);

  if (state.shouldExist) {
    bool out_of_sync = !state.inOverlay ||
        state.overlayDtype != state.desiredDtype ||
        state.overlayHash.has_value() != state.desiredHash.has_value() ||
        (state.overlayHash.has_value() &&
         !state.overlayHash.value().bytesEqual(state.desiredHash.value()));
    if (out_of_sync) {
      XLOG(DBG9, "Out of sync: adding/updating entry");
      XLOGF(
          DBG9,
          "overlayDtype={} vs desiredDtype={}, overlayHash={} vs desiredHash={}",
          state.overlayDtype,
          state.desiredDtype,
          state.overlayHash->toLogString(),
          state.desiredHash->toLogString());
      if (state.inOverlay && state.overlayDtype != state.desiredDtype) {
        // If the file/directory type doesn't match, remove the old entry
        // entirely, since we need to recursively remove a directory in order to
        // write a file, and vice versa.
        removeOverlayEntry(overlay, parentInodeNum, name, *state.overlayEntry);
      }

      return addOrUpdateOverlay(
          overlay,
          parentInodeNum,
          name,
          state.desiredDtype,
          state.desiredHash,
          insensitiveOverlayDir);
    } else {
      return InodeNumber(*state.overlayEntry->inodeNumber());
    }
  } else {
    if (state.inOverlay) {
      XLOG(DBG9, "Out of sync: removing extra");
      removeOverlayEntry(overlay, parentInodeNum, name, *state.overlayEntry);
    }
    return std::nullopt;
  }
}

// Returns true if the given path is considered materialized.
bool processChildren(
    TreeOverlay& overlay,
    RelativePathPiece path,
    AbsolutePathPiece root,
    InodeNumber inodeNumber,
    const PathMap<overlay::OverlayEntry>& insensitiveOverlayDir,
    const std::shared_ptr<const Tree>& scmTree,
    const TreeOverlay::LookupCallback& callback,
    uint64_t logFrequency,
    uint64_t& traversedDirectories) {
  XLOGF(DBG9, "processChildren - {}", path);

  traversedDirectories++;
  if (traversedDirectories % logFrequency == 0) {
    // TODO: We could also report the progress to the StartupLogger to be
    // displayed in the user console. That however requires a percent and it's
    // a bit unclear how we can compute this percent.
    XLOGF(INFO, "{} directories scanned", traversedDirectories);
  }

  // Handle children
  folly::F14NodeMap<std::string, FsckFileState> children;

  // Populate children disk information
  auto absPath = root + path;
  WIN32_FIND_DATAW findFileData;
  std::wstring longPath{L"\\\\?\\"};
  longPath += (absPath + "*"_relpath).wide();
  HANDLE h = FindFirstFileExW(
      longPath.c_str(),
      FindExInfoBasic,
      &findFileData,
      FindExSearchNameMatch,
      nullptr,
      0);
  if (h == INVALID_HANDLE_VALUE) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }

  do {
    if (wcscmp(findFileData.cFileName, L".") == 0 ||
        wcscmp(findFileData.cFileName, L"..") == 0 ||
        wcscmp(findFileData.cFileName, L".eden") == 0) {
      continue;
    }
    PathComponent name{findFileData.cFileName};
    auto& childState = children[name.stringPiece()];
    populateDiskState(root, path + name, childState, findFileData);
  } while (FindNextFileW(h, &findFileData) != 0);

  auto error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }

  FindClose(h);

  // Populate children overlay information
  for (const auto& [name, overlayEntry] : insensitiveOverlayDir) {
    auto& childState = children[name.stringPiece()];
    populateOverlayState(childState, overlayEntry);
  }

  // Don't recurse if there are no disk children for fixing up or overlay
  // children for deleting.
  if (children.empty()) {
    return false;
  }

  // Populate children scm information
  if (scmTree) {
    for (const auto& [name, treeEntry] : *scmTree) {
      PathComponentPiece pathName{name};
      auto& childState = children[pathName.stringPiece()];
      populateScmState(childState, treeEntry);
    }
  }

  // Recurse for any children.
  bool anyChildMaterialized = false;
  for (auto& [name, childState] : children) {
    auto childName = PathComponentPiece{name};
    auto childPath = path + childName;
    XLOGF(DBG9, "process child - {}", childPath);

    std::optional<InodeNumber> childInodeNumberOpt = fixup(
        childState, overlay, childPath, inodeNumber, insensitiveOverlayDir);

    anyChildMaterialized |= childState.diskMaterialized;

    if (childState.desiredDtype == dtype_t::Dir && childState.onDisk &&
        !childState.diskEmptyPlaceholder && childInodeNumberOpt.has_value()) {
      // Fetch child scm tree.
      std::shared_ptr<const Tree> childScmTree;
      if (childState.scmDtype == dtype_t::Dir) {
        // TODO: handle scm failure
        auto scmEntryTry = callback(childPath).getTry();
        std::variant<
            std::shared_ptr<const facebook::eden::Tree>,
            facebook::eden::TreeEntry>& childScmEntry = scmEntryTry.value();
        // It's guaranteed to be a Tree since scmDtype is Dir.
        childScmTree = std::get<std::shared_ptr<const Tree>>(childScmEntry);
      }

      auto childInodeNumber = *childInodeNumberOpt;
      auto childOverlayDir = *overlay.loadOverlayDir(childInodeNumber);
      auto childInsensitiveOverlayDir = toPathMap(childOverlayDir);
      bool childMaterialized = childState.diskMaterialized;
      childMaterialized |= processChildren(
          overlay,
          childPath,
          root,
          childInodeNumber,
          childInsensitiveOverlayDir,
          childScmTree,
          callback,
          logFrequency,
          traversedDirectories);
      anyChildMaterialized |= childMaterialized;

      if (childMaterialized && childState.desiredHash != std::nullopt) {
        XLOGF(
            DBG9,
            "Directory {} has a materialized child, and therefore is materialized too. Marking.",
            childPath);
        childState.diskMaterialized = true;
        childState.desiredHash = std::nullopt;
        // Refresh the parent state so we see and update the current overlay
        // entry.
        auto updatedOverlayDir = *overlay.loadOverlayDir(inodeNumber);
        auto updatedInsensitiveOverlayDir = toPathMap(updatedOverlayDir);
        // Update the overlay entry to remove the scmHash.
        addOrUpdateOverlay(
            overlay,
            inodeNumber,
            childName,
            childState.desiredDtype,
            childState.desiredHash,
            updatedInsensitiveOverlayDir);
      }
    }
  }

  return anyChildMaterialized;
}

void scanCurrentDir(
    TreeOverlay& overlay,
    AbsolutePathPiece dir,
    InodeNumber inode,
    const overlay::OverlayDir& parentOverlayDir,
    const PathMap<overlay::OverlayEntry>& parentInsensitiveOverlayDir,
    bool recordDeletion,
    TreeOverlay::LookupCallback& callback) {
  auto boostPath = boost::filesystem::path(dir.stringPiece());
  if (!boost::filesystem::is_directory(boostPath)) {
    XLOGF(WARN, "Attempting to scan '{}' which is not a directory", dir);
    return;
  }

  XLOGF(DBG3, "Scanning {}", dir);

  auto overlayEntries = makeEntriesSet(parentOverlayDir);
  // Loop to synchronize overlay state with disk state
  for (const auto& entry : boost::filesystem::directory_iterator(boostPath)) {
    auto path = AbsolutePath{entry.path().c_str()};
    auto name = path.basename();
    auto dtype = dtypeFromPath(entry.path());

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
      auto overlayEntry =
          getEntryFromOverlayDir(parentInsensitiveOverlayDir, name);
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
        removeOverlayEntry(overlay, inode, name, *overlayEntry);
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
    for (auto removed = overlayEntries.begin(); removed != overlayEntries.end();
         removed++) {
      XLOGF(DBG3, "Removing missing entry from overlay: {}", *removed);
      auto overlayEntry =
          getEntryFromOverlayDir(parentInsensitiveOverlayDir, *removed);
      removeOverlayEntry(overlay, inode, *removed, *overlayEntry);
    }
  }

  XLOGF(DBG9, "Reloading {} from overlay.", inode);
  // Reload the updated overlay as we have fixed the inconsistency.
  auto updated = *overlay.loadOverlayDir(inode);
  auto updatedInsensitiveOverlayDir = toPathMap(updated);

  // Now that this overlay directory is consistent with the on-disk state,
  // proceed to its children.
  for (const auto& entry : boost::filesystem::directory_iterator(boostPath)) {
    auto path = AbsolutePath{entry.path().c_str()};
    auto mode = dtypeFromPath(entry.path());

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
      auto overlayEntry =
          getEntryFromOverlayDir(updatedInsensitiveOverlayDir, path.basename());
      auto entryInode =
          InodeNumber::fromThrift(*overlayEntry->inodeNumber_ref());
      auto entryDir = *overlay.loadOverlayDir(entryInode);
      auto entryInsensitiveOverlayDir = toPathMap(entryDir);
      scanCurrentDir(
          overlay,
          path,
          entryInode,
          entryDir,
          entryInsensitiveOverlayDir,
          isFull,
          callback);
    }
  }
}
} // namespace

void windowsFsckScanLocalChanges(
    std::shared_ptr<const EdenConfig> config,
    TreeOverlay& overlay,
    AbsolutePathPiece mountPath,
    TreeOverlay::LookupCallback& callback) {
  XLOGF(INFO, "Start scanning {}", mountPath);
  if (auto view = overlay.loadOverlayDir(kRootNodeId)) {
    auto insensitiveOverlayDir = toPathMap(*view);
    if (config->useThoroughFsck.getValue()) {
      // TODO: Handler errors or no trees
      auto scmEntryTry = callback(""_relpath).getTry();
      std::variant<
          std::shared_ptr<const facebook::eden::Tree>,
          facebook::eden::TreeEntry>& scmEntry = scmEntryTry.value();
      std::shared_ptr<const Tree> scmTree =
          std::get<std::shared_ptr<const Tree>>(scmEntry);
      uint64_t traversedDirectories = 1;
      processChildren(
          overlay,
          ""_relpath,
          mountPath,
          kRootNodeId,
          insensitiveOverlayDir,
          scmTree,
          callback,
          config->fsckLogFrequency.getValue(),
          traversedDirectories);
    } else {
      scanCurrentDir(
          overlay,
          mountPath,
          kRootNodeId,
          *view,
          insensitiveOverlayDir,
          false,
          callback);
    }
    XLOGF(INFO, "Scanning complete for {}", mountPath);
  } else {
    XLOG(INFO)
        << "Unable to start fsck since root inode is not present. Possibly new mount.";
  }
}

} // namespace facebook::eden

#endif
