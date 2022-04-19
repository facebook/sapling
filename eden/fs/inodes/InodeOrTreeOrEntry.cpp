/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeOrTreeOrEntry.h"

#include "eden/common/utils/Synchronized.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/StatTimes.h"

namespace facebook::eden {

using detail::TreePtr;

InodePtr InodeOrTreeOrEntry::asInodePtr() const {
  return std::get<InodePtr>(variant_);
}

// Helper template for std::visit calls below
template <class>
inline constexpr bool always_false_v = false;

dtype_t InodeOrTreeOrEntry::getDtype() const {
  return std::visit(
      [](auto&& arg) {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return arg->getType();
        } else if constexpr (std::is_same_v<
                                 T,
                                 UnmaterializedUnloadedBlobDirEntry>) {
          return arg.getDtype();
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return dtype_t::Dir;
        } else if constexpr (std::is_same_v<T, TreeEntry>) {
          return arg.getDType();
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<Hash20> InodeOrTreeOrEntry::getSHA1(
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  // Ensure this is a regular file.
  // We intentionally want to refuse to compute the SHA1 of symlinks
  switch (getDtype()) {
    case dtype_t::Dir:
      return makeImmediateFuture<Hash20>(PathError(EISDIR, path));
    case dtype_t::Symlink:
      return makeImmediateFuture<Hash20>(
          PathError(EINVAL, path, "file is a symlink"));
    case dtype_t::Regular:
      break;
    default:
      return makeImmediateFuture<Hash20>(
          PathError(EINVAL, path, "variant is of unhandled type"));
  }

  // This is now guaranteed to be a dtype_t::Regular file. This means there's no
  // need for a Tree case, as Trees are always directories.

  return std::visit(
      [path, objectStore, &fetchContext](
          auto&& arg) -> ImmediateFuture<Hash20> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return arg.asFilePtr()->getSha1(fetchContext);
        } else if constexpr (std::is_same_v<
                                 T,
                                 UnmaterializedUnloadedBlobDirEntry>) {
          return objectStore->getBlobSha1(arg.getHash(), fetchContext);
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return makeImmediateFuture<Hash20>(PathError(EISDIR, path));
        } else if constexpr (std::is_same_v<T, TreeEntry>) {
          const auto& hash = arg.getContentSha1();
          // If available, use the TreeEntry's ContentsSha1
          if (hash.has_value()) {
            return ImmediateFuture<Hash20>(hash.value());
          }
          // Revert to querying the objectStore for the file's medatadata
          return objectStore->getBlobSha1(arg.getHash(), fetchContext);
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<BlobMetadata> InodeOrTreeOrEntry::getBlobMetadata(
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  // Ensure this is a regular file.
  // We intentionally want to refuse to compute the SHA1 of symlinks
  switch (getDtype()) {
    case dtype_t::Dir:
      return makeImmediateFuture<BlobMetadata>(PathError(EISDIR, path));
    case dtype_t::Symlink:
      return makeImmediateFuture<BlobMetadata>(
          PathError(EINVAL, path, "file is a symlink"));
    case dtype_t::Regular:
      break;
    default:
      return makeImmediateFuture<BlobMetadata>(
          PathError(EINVAL, path, "variant is of unhandled type"));
  }

  // This is now guaranteed to be a dtype_t::Regular file. This means there's no
  // need for a Tree case, as Trees are always directories. It's included to
  // check that the visitor here is exhaustive.
  return std::visit(
      [path, objectStore, &fetchContext](
          auto&& arg) -> ImmediateFuture<BlobMetadata> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return arg.asFilePtr()->getBlobMetadata(fetchContext);
        } else if constexpr (
            std::is_same_v<T, UnmaterializedUnloadedBlobDirEntry> ||
            std::is_same_v<T, TreeEntry>) {
          return objectStore->getBlobMetadata(arg.getHash(), fetchContext);
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return makeImmediateFuture<BlobMetadata>(PathError(EISDIR, path));
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

// Returns a subset of `struct stat` required by
// EdenServiceHandler::semifuture_getFileInformation()
ImmediateFuture<struct stat> InodeOrTreeOrEntry::stat(
    const struct timespec& lastCheckoutTime,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  return std::visit(
      [ lastCheckoutTime, treeMode = treeMode_, objectStore, &
        fetchContext ](auto&& arg) -> ImmediateFuture<struct stat> {
        using T = std::decay_t<decltype(arg)>;
        ObjectId hash;
        mode_t mode;
        if constexpr (std::is_same_v<T, InodePtr>) {
          // Note: there's no need to modify the return value of stat here, as
          // the inode implementations are what all the other cases are trying
          // to emulate.
          return arg->stat(fetchContext);
        } else if constexpr (std::is_same_v<
                                 T,
                                 UnmaterializedUnloadedBlobDirEntry>) {
          hash = arg.getHash();
          mode = arg.getInitialMode();
          // fallthrough
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          struct stat st = {};
          st.st_mode = static_cast<decltype(st.st_mode)>(treeMode);
          stMtime(st, lastCheckoutTime);
#ifdef _WIN32
          // Windows returns zero for st_mode and mtime
          st.st_mode = static_cast<decltype(st.st_mode)>(0);
          {
            struct timespec ts0 {};
            stMtime(st, ts0);
          }
#endif
          st.st_size = 0U;
          return ImmediateFuture{st};
        } else if constexpr (std::is_same_v<T, TreeEntry>) {
          hash = arg.getHash();
          mode = modeFromTreeEntryType(arg.getType());
          // fallthrough
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
        return objectStore->getBlobMetadata(hash, fetchContext)
            .thenValue([mode, lastCheckoutTime](const BlobMetadata& metadata) {
              struct stat st = {};
              st.st_mode = static_cast<decltype(st.st_mode)>(mode);
              stMtime(st, lastCheckoutTime);
#ifdef _WIN32
              // Windows returns zero for st_mode and mtime
              st.st_mode = static_cast<decltype(st.st_mode)>(0);
              {
                struct timespec ts0 {};
                stMtime(st, ts0);
              }
#endif
              st.st_size = static_cast<decltype(st.st_size)>(metadata.size);
              return st;
            });
      },
      variant_);
}

ImmediateFuture<InodeOrTreeOrEntry> InodeOrTreeOrEntry::getOrFindChild(
    PathComponentPiece childName,
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  if (!isDirectory()) {
    return makeImmediateFuture<InodeOrTreeOrEntry>(PathError(ENOTDIR, path));
  }
  return std::visit(
      [childName, path, objectStore, &fetchContext](
          auto&& arg) -> ImmediateFuture<InodeOrTreeOrEntry> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return arg.asTreePtr()->getOrFindChild(
              childName, fetchContext, false);
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return getOrFindChild(
              arg, childName, path, objectStore, fetchContext);
        } else if constexpr (
            std::is_same_v<T, UnmaterializedUnloadedBlobDirEntry> ||
            std::is_same_v<T, TreeEntry>) {
          // These represent files in InodeOrTreeOrEntry, and can't be descended
          return makeImmediateFuture<InodeOrTreeOrEntry>(
              PathError(ENOTDIR, path, "variant is of unhandled type"));
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<InodeOrTreeOrEntry> InodeOrTreeOrEntry::getOrFindChild(
    TreePtr tree,
    PathComponentPiece childName,
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) {
  // Lookup the next child
  auto* treeEntry = tree->getEntryPtr(childName);
  if (!treeEntry) {
    // Note that the path printed below is the requested path that is being
    // walked, childName may appear anywhere in the path.
    XLOG(DBG7) << "attempted to find non-existent TreeEntry \"" << childName
               << "\" in " << path;
    return makeImmediateFuture<InodeOrTreeOrEntry>(
        std::system_error(ENOENT, std::generic_category()));
  }

  // Always descend if the treeEntry is a Tree
  if (treeEntry->isTree()) {
    return objectStore->getTree(treeEntry->getHash(), fetchContext)
        .thenValue(
            [mode = modeFromTreeEntryType(treeEntry->getType())](TreePtr tree) {
              return InodeOrTreeOrEntry{std::move(tree), mode};
            });
  } else {
    // This is a file, return the TreeEntry for it
    return ImmediateFuture{InodeOrTreeOrEntry{*treeEntry}};
  }
}

} // namespace facebook::eden
