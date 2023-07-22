/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/sqlitecatalog/WindowsFsck.h"

#ifdef _WIN32
#include <folly/executors/SerialExecutor.h>
#include <folly/portability/Windows.h>

#include <winioctl.h> // @manual

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/WinError.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/ProjfsUtil.h"

namespace facebook::eden {
namespace {

// TODO
// - test/fix behavior when offline

// Reparse tag for UNIX domain socket is not defined in Windows header files.
const ULONG IO_REPARSE_TAG_SOCKET = 0x80000023;

dtype_t dtypeFromAttrs(DWORD dwFileAttributes, DWORD dwReserved0) {
  if (dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) {
    // Microsoft documents the dwReserved0 member as holding the reparse tag:
    // https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-win32_find_dataw
    if (dwReserved0 == IO_REPARSE_TAG_SYMLINK ||
        dwReserved0 == IO_REPARSE_TAG_MOUNT_POINT) {
      return dtype_t::Symlink;
    } else if (dwReserved0 == IO_REPARSE_TAG_SOCKET) {
      return dtype_t::Socket;
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

void removeChildRecursively(InodeCatalog& inodeCatalog, InodeNumber inode) {
  XLOGF(DBG9, "Removing directory inode = {} ", inode);
  if (auto dir = inodeCatalog.loadOverlayDir(inode)) {
    const auto& entries = dir->entries_ref();
    for (auto iter = entries->cbegin(); iter != entries->cend(); iter++) {
      const auto& entry = iter->second;
      if (S_ISDIR(*entry.mode_ref())) {
        auto entryInode = InodeNumber::fromThrift(*entry.inodeNumber_ref());
        removeChildRecursively(inodeCatalog, entryInode);
      }
      XLOGF(DBG9, "Removing child path = {}", iter->first);
      inodeCatalog.removeChild(inode, PathComponentPiece{iter->first});
    }
  }
}

// Remove entry from inodeCatalog, but recursively if the entry is a directory.
// This is different from `inodeCatalog.removeChild` as it does not remove
// directory recursively.
void removeOverlayEntry(
    InodeCatalog& inodeCatalog,
    InodeNumber parent,
    PathComponentPiece name,
    const overlay::OverlayEntry& entry) {
  XLOGF(DBG9, "Remove overlay entry: {}", name);
  auto overlayMode = static_cast<mode_t>(*entry.mode_ref());
  if (S_ISDIR(overlayMode)) {
    auto overlayInode = InodeNumber::fromThrift(*entry.inodeNumber_ref());
    removeChildRecursively(inodeCatalog, overlayInode);
  }
  inodeCatalog.removeChild(parent, name);
}

// clang-format off
// T = tombstone
//
// for path in union(onDisk_paths, inOverlay_paths, inScm_paths):
//   disk  overlay  scm   action
//    y       n      n      add to inodeCatalog, no scm hash.   (If is_placeholder() error since there's no scm to fill it? We could call PrjDeleteFile on it.)
//    y       y      n      fix overlay mode_t to match disk if necessary. (If is_placeholder(), error since there's no scm to fill it?)
//    y       n      y      add to inodeCatalog, use scm hash if placeholder-file or empty-placeholder-directory.
//    y       y      y      fix overlay mode_t to match disk if necessary
//    T       n      *      do nothing
//    T       y      *      drop from inodeCatalog, recursively
//    n       y      n      remove from overlay
//    n       y      y      fix overlay mode_t to match scm if necessary.
//    n       n      y      add to inodeCatalog, use scm hash
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
  // populatedOrFullOrTomb is true if:
  //  - a file is full, hydrated or tomstoned
  //  - a directory is full or a dirty placeholder or a descendant is
  //  populatedOrFullOrTomb
  bool populatedOrFullOrTomb = false;
  // diskEmptyPlaceholder is true if:
  //  - a file is virtual or a placeholder
  //  - a directory is a placeholder and has no children (placeholder or
  //  otherwise)

  bool renamedPlaceholder = false;

  bool diskEmptyPlaceholder = false;
  bool directoryIsFull = false;
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
    throw std::runtime_error(fmt::format(
        "unable to check directory - {}",
        wideToMultibyteString<std::string>(path)));
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
    throw std::runtime_error(fmt::format(
        "unable to check directory - {}",
        wideToMultibyteString<std::string>(path)));
  }

  FindClose(h);
  return true;
}

void populateDiskState(
    AbsolutePathPiece root,
    RelativePathPiece path,
    FsckFileState& state,
    const WIN32_FIND_DATAW& findFileData,
    bool fsckRenamedFiles) {
  dtype_t dtype =
      dtypeFromAttrs(findFileData.dwFileAttributes, findFileData.dwReserved0);
  if (dtype != dtype_t::Dir && dtype != dtype_t::Regular) {
    state.onDisk = true;
    // On Windows, EdenFS consider all special files (symlinks, sockets, etc)
    // to be regular.
    state.diskDtype = dtype_t::Regular;
    state.populatedOrFullOrTomb = true;
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
  auto sparse = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_SPARSE_FILE) ==
      FILE_ATTRIBUTE_SPARSE_FILE;

  bool detectedTombstone = reparse && !recall && hidden && system;
  bool detectedFull = !reparse && !recall;

  state.onDisk = true;
  state.diskDtype = dtype;
  state.diskTombstone = detectedTombstone;

  // It can also be populated if a descendant directory is
  // materialized. But that is checked later when processing the children.
  state.populatedOrFullOrTomb = detectedFull || detectedTombstone;
  // It's an empty placeholder unless it's materialized or it has children.
  state.diskEmptyPlaceholder = !state.populatedOrFullOrTomb;
  state.directoryIsFull = !recall;

  state.renamedPlaceholder = false;

  if (fsckRenamedFiles && sparse) {
    auto renamedPlaceholderResult =
        isRenamedPlaceholder((root + path).wide().c_str());
    if (renamedPlaceholderResult.hasValue()) {
      state.renamedPlaceholder = renamedPlaceholderResult.value();
    } else {
      XLOG(DBG9) << "Error checking rename: "
                 << renamedPlaceholderResult.exception();
    }
  }

  if (dtype == dtype_t::Dir) {
    auto absPath = root + path;
    auto wPath = absPath.wide();
    if (!directoryIsEmpty(wPath.c_str())) {
      state.diskEmptyPlaceholder = false;
    }
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
    InodeCatalog& inodeCatalog,
    InodeNumber parentInodeNum,
    PathComponentPiece name,
    dtype_t dtype,
    std::optional<ObjectId> hash,
    const PathMap<overlay::OverlayEntry>& parentInsensitiveOverlayDir) {
  if (inodeCatalog.hasChild(parentInodeNum, name)) {
    XLOGF(DBG9, "Updating overlay: {}", name);
    inodeCatalog.removeChild(parentInodeNum, name);
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
    overlayEntry.inodeNumber() = inodeCatalog.nextInodeNumber().get();
  }
  if (hash.has_value()) {
    overlayEntry.hash() = hash->asString();
  } else {
    overlayEntry.hash().reset();
  }
  overlayEntry.mode() = dtype_to_mode(dtype);
  inodeCatalog.addChild(parentInodeNum, name, overlayEntry);
  return InodeNumber(*overlayEntry.inodeNumber());
}

enum class DirectoryOnDiskState { Full, Placeholder };

std::optional<InodeNumber> fixup(
    FsckFileState& state,
    InodeCatalog& inodeCatalog,
    RelativePathPiece path,
    InodeNumber parentInodeNum,
    const PathMap<overlay::OverlayEntry>& insensitiveOverlayDir,
    DirectoryOnDiskState parentProjFSState) {
  auto name = path.basename();

  if (!state.onDisk) {
    if (parentProjFSState == DirectoryOnDiskState::Full) {
      // state.shouldExist defaults to false
    } else if (state.inScm) {
      state.desiredDtype = state.scmDtype;
      state.desiredHash = state.scmHash;
      state.shouldExist = true;
    }
  } else if (state.diskTombstone) {
    // state.shouldExist defaults to false
  } else if (state.renamedPlaceholder && !state.populatedOrFullOrTomb) {
    // renamed files are special snowflakes in EdenFS, they are the only inodes
    // that can be regular placeholders in projfs and represented by
    // materialized inodes on disk.
    state.desiredDtype = state.diskDtype;
    state.desiredHash =
        std::nullopt; // renamed files should always be materialized in EdenFS.
    // This could cause hg status and hg diff to make recersive calls in EdenFS,
    // but this is ok because the read will be served out of source control
    // (i.e. no infinite recursion yay!). and eden knows how to make sure these
    // things don't happen on the same thread (i.e. no deadlock double yay!).
    state.shouldExist = true;
  } else { // if file exists normally on disk
    if (!state.inScm && !state.populatedOrFullOrTomb) {
      // Stop fixing this up since we can't materialize if it's not in scm.
      // (except for when it's a renamed file, see the case above)
      // TODO: This is likely caused by EdenFS not having called PrjDeleteFile
      // in a previous checkout operation. We should probably call it here or
      // as a post-PrjfsChannel initialization.

      XLOGF(DFATAL, "Placeholder present on disk but not in SCM - {}", path);
      return std::nullopt;
    } else {
      state.desiredDtype = state.diskDtype;
      state.desiredHash =
          state.populatedOrFullOrTomb ? std::nullopt : state.scmHash;
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
      state.populatedOrFullOrTomb);

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
          state.overlayHash ? state.overlayHash->toLogString() : "<null>",
          state.desiredHash ? state.desiredHash->toLogString() : "<null>");
      if (state.inOverlay && state.overlayDtype != state.desiredDtype) {
        // If the file/directory type doesn't match, remove the old entry
        // entirely, since we need to recursively remove a directory in order to
        // write a file, and vice versa.
        removeOverlayEntry(
            inodeCatalog, parentInodeNum, name, *state.overlayEntry);
      }

      return addOrUpdateOverlay(
          inodeCatalog,
          parentInodeNum,
          name,
          state.desiredDtype,
          state.desiredHash,
          insensitiveOverlayDir);
    } else {
      auto inodeNumber = InodeNumber(*state.overlayEntry->inodeNumber());
      if (!state.onDisk && state.overlayDtype == dtype_t::Dir) {
        auto overlayDir = inodeCatalog.loadAndRemoveOverlayDir(inodeNumber);
        if (overlayDir) {
          XLOGF(DBG9, "Removed overlay directory for: {}", path);
        }
      }
      return inodeNumber;
    }
  } else {
    if (state.inOverlay) {
      XLOG(DBG9, "Out of sync: removing extra");
      removeOverlayEntry(
          inodeCatalog, parentInodeNum, name, *state.overlayEntry);
    }
    return std::nullopt;
  }
}

/**
 * List all the on-disk entries and return a PathMap from them.
 */
PathMap<FsckFileState> populateOnDiskChildrenState(
    AbsolutePathPiece root,
    RelativePathPiece path,
    bool fsckRenamedFiles) {
  PathMap<FsckFileState> children{CaseSensitivity::Insensitive};
  auto absPath = (root + path + "*"_pc).wide();

  WIN32_FIND_DATAW findFileData;
  // TODO: Should FIND_FIRST_EX_ON_DISK_ENTRIES_ONLY be used?
  HANDLE h = FindFirstFileExW(
      absPath.c_str(),
      FindExInfoBasic,
      &findFileData,
      FindExSearchNameMatch,
      nullptr,
      0);
  if (h == INVALID_HANDLE_VALUE) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }
  SCOPE_EXIT {
    FindClose(h);
  };

  do {
    if (wcscmp(findFileData.cFileName, L".") == 0 ||
        wcscmp(findFileData.cFileName, L"..") == 0 ||
        wcscmp(findFileData.cFileName, L".eden") == 0) {
      continue;
    }
    PathComponent name{findFileData.cFileName};
    auto& childState = children[name];
    populateDiskState(
        root, path + name, childState, findFileData, fsckRenamedFiles);
  } while (FindNextFileW(h, &findFileData) != 0);

  auto error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }

  return children;
}

/**
 * Recursively crawl the path rooted at root / path.
 *
 * Returns true if the given path is either populated or full or a tombstone.
 *
 * The caller must ensure that the inodeCatalog, the root path, the callback
 * and the traversedDirectories live longer than the returned future. As for
 * the path and scmTree argument, this function will copy them if needed.
 */
ImmediateFuture<bool> processChildren(
    InodeCatalog& inodeCatalog,
    RelativePathPiece path,
    AbsolutePathPiece root,
    InodeNumber inodeNumber,
    const PathMap<overlay::OverlayEntry>& insensitiveOverlayDir,
    const std::shared_ptr<const Tree>& scmTree,
    const InodeCatalog::LookupCallback& callback,
    uint64_t logFrequency,
    std::atomic<uint64_t>& traversedDirectories,
    bool fsckRenamedFiles,
    DirectoryOnDiskState parentOnDiskState) {
  XLOGF(DBG9, "processChildren - {}", path);

  auto traversed = traversedDirectories.fetch_add(1, std::memory_order_relaxed);
  if (traversed % logFrequency == 0) {
    // TODO: We could also report the progress to the StartupLogger to be
    // displayed in the user console. That however requires a percent and it's
    // a bit unclear how we can compute this percent.
    XLOGF(INFO, "{} directories scanned", traversed);
  }

  auto children = populateOnDiskChildrenState(root, path, fsckRenamedFiles);

  for (const auto& [name, overlayEntry] : insensitiveOverlayDir) {
    auto& childState = children[name];
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
      auto& childState = children[name];
      populateScmState(childState, treeEntry);
    }
  }

  std::vector<ImmediateFuture<folly::Unit>> childFutures;
  childFutures.reserve(children.size());

  // Recurse for any children.
  for (auto& [childName, childState] : children) {
    auto childPath = path + childName;
    XLOGF(DBG9, "process child - {}", childPath);

    std::optional<InodeNumber> childInodeNumberOpt = fixup(
        childState,
        inodeCatalog,
        childPath,
        inodeNumber,
        insensitiveOverlayDir,
        parentOnDiskState);

    if (childState.desiredDtype == dtype_t::Dir && childState.onDisk &&
        !childState.diskEmptyPlaceholder && childInodeNumberOpt.has_value()) {
      // Fetch child scm tree.
      ImmediateFuture<std::shared_ptr<const Tree>> childScmTreeFut{
          std::in_place};
      if (childState.scmDtype == dtype_t::Dir) {
        // Move the callback to a non-ready ImmediateFuture to make sure that
        // the disk crawling is performed in a different thread (ie:
        // not-immediately) in the case where the Tree is in the hgcache
        // already.
        childScmTreeFut =
            makeNotReadyImmediateFuture()
                .thenValue(
                    [&callback, scmTree, childName = RelativePath{childName}](
                        auto&&) { return callback(scmTree, childName); })
                .thenValue(
                    [](std::variant<std::shared_ptr<const Tree>, TreeEntry>
                           scmEntry) {
                      // TODO: handle scm failure
                      // It's guaranteed to be a Tree since scmDtype is Dir.
                      return std::move(
                          std::get<std::shared_ptr<const Tree>>(scmEntry));
                    });
      }

      childFutures.emplace_back(
          std::move(childScmTreeFut)
              .thenValue([&inodeCatalog,
                          isFull = childState.directoryIsFull,
                          childPath = childPath.copy(),
                          root,
                          &callback,
                          logFrequency,
                          &traversedDirectories,
                          childInodeNumber = *childInodeNumberOpt,
                          fsckRenamedFiles](
                             const std::shared_ptr<const Tree>& childScmTree) {
                auto childOverlayDir =
                    *inodeCatalog.loadOverlayDir(childInodeNumber);
                auto childInsensitiveOverlayDir = toPathMap(childOverlayDir);

                return processChildren(
                    inodeCatalog,
                    childPath,
                    root,
                    childInodeNumber,
                    childInsensitiveOverlayDir,
                    childScmTree,
                    callback,
                    logFrequency,
                    traversedDirectories,
                    fsckRenamedFiles,
                    isFull ? DirectoryOnDiskState::Full
                           : DirectoryOnDiskState::Placeholder);
              })
              .thenValue([&childState = childState,
                          childPath = childPath.copy(),
                          &inodeCatalog,
                          inodeNumber](bool childPopulatedOrFullOrTomb) {
                childState.populatedOrFullOrTomb |= childPopulatedOrFullOrTomb;

                if (childPopulatedOrFullOrTomb &&
                    childState.desiredHash != std::nullopt) {
                  XLOGF(
                      DBG9,
                      "Directory {} has a materialized child, and therefore is materialized too. Marking.",
                      childPath);
                  childState.desiredHash = std::nullopt;

                  auto updatedOverlayDir =
                      *inodeCatalog.loadOverlayDir(inodeNumber);
                  auto updatedInsensitiveOverlayDir =
                      toPathMap(updatedOverlayDir);
                  // Update the overlay entry to remove the scmHash.
                  addOrUpdateOverlay(
                      inodeCatalog,
                      inodeNumber,
                      childPath.basename(),
                      childState.desiredDtype,
                      childState.desiredHash,
                      updatedInsensitiveOverlayDir);
                }
                return folly::unit;
              }));
    }
  }

  return collectAllSafe(std::move(childFutures))
      // The futures have references on this PathMap, make sure it stays alive.
      .thenValue([children = std::move(children)](auto&&) {
        for (const auto& [childName, childState] : children) {
          if (childState.populatedOrFullOrTomb) {
            return true;
          }
        }
        return false;
      });
}

} // namespace

void windowsFsckScanLocalChanges(
    std::shared_ptr<const EdenConfig> config,
    InodeCatalog& inodeCatalog,
    AbsolutePathPiece mountPath,
    InodeCatalog::LookupCallback& callback) {
  XLOGF(INFO, "Start scanning {}", mountPath);
  if (auto view = inodeCatalog.loadOverlayDir(kRootNodeId)) {
    auto insensitiveOverlayDir = toPathMap(*view);
    std::atomic<uint64_t> traversedDirectories = 1;
    // TODO: Handler errors or no trees

    auto executor = folly::getGlobalCPUExecutor();
    if (!config->multiThreadedFsck.getValue()) {
      executor = folly::SerialExecutor::create();
    }

    folly::via(
        executor,
        [&callback]() { return callback(nullptr, ""_relpath).semi(); })
        .thenValue(
            [&inodeCatalog,
             mountPath,
             insensitiveOverlayDir = std::move(insensitiveOverlayDir),
             &traversedDirectories,
             &callback,
             logFrequency = config->fsckLogFrequency.getValue(),
             fsckRenamedFiles = config->prjfsFsckDetectRenames.getValue()](
                std::variant<std::shared_ptr<const Tree>, TreeEntry> scmEntry) {
              auto scmTree =
                  std::get<std::shared_ptr<const Tree>>(std::move(scmEntry));
              return processChildren(
                         inodeCatalog,
                         ""_relpath,
                         mountPath,
                         kRootNodeId,
                         insensitiveOverlayDir,
                         scmTree,
                         callback,
                         logFrequency,
                         traversedDirectories,
                         fsckRenamedFiles,
                         DirectoryOnDiskState::Placeholder)
                  .semi();
            })
        .get();
    XLOGF(INFO, "Scanning complete for {}", mountPath);
  } else {
    XLOG(INFO)
        << "Unable to start fsck since root inode is not present. Possibly new mount.";
  }
}

} // namespace facebook::eden

#endif
