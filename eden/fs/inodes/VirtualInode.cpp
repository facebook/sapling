/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/VirtualInode.h"

#include "eden/common/utils/Synchronized.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/StatTimes.h"

namespace facebook::eden {

using detail::TreePtr;

InodePtr VirtualInode::asInodePtr() const {
  return std::get<InodePtr>(variant_);
}

// Helper template for std::visit calls below
template <class>
inline constexpr bool always_false_v = false;

dtype_t VirtualInode::getDtype() const {
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

bool VirtualInode::isDirectory() const {
  return getDtype() == dtype_t::Dir;
}

VirtualInode::ContainedType VirtualInode::testGetContainedType() const {
  return std::visit(
      [](auto&& arg) {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return ContainedType::Inode;
        } else if constexpr (std::is_same_v<
                                 T,
                                 UnmaterializedUnloadedBlobDirEntry>) {
          return ContainedType::DirEntry;
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return ContainedType::Tree;
        } else if constexpr (std::is_same_v<T, TreeEntry>) {
          return ContainedType::TreeEntry;
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<Hash20> VirtualInode::getSHA1(
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

ImmediateFuture<TreeEntryType> VirtualInode::getTreeEntryType(
    RelativePathPiece path,
    ObjectFetchContext& fetchContext) const {
  return std::visit(
      [&fetchContext, path](auto&& arg) -> ImmediateFuture<TreeEntryType> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
#ifdef _WIN32
          (void)fetchContext;
          // stat does not have real data for an inode on Windows, so we can not
          // directly use the mode bits. Further inodes are only tree or regular
          // files on windows see treeEntryTypeFromMode.
          switch (arg->getType()) {
            case dtype_t::Dir:
              return TreeEntryType::TREE;
            case dtype_t::Regular:
              return TreeEntryType::REGULAR_FILE;
            default:
              return makeImmediateFuture<TreeEntryType>(
                  PathError(EINVAL, path, "variant is of unhandled type"));
          }
#else
          (void)path;
          return arg->stat(fetchContext).thenValue([](const struct stat&& st) {
            return treeEntryTypeFromMode(st.st_mode).value();
          });
#endif
        } else if constexpr (std::is_same_v<
                                 T,
                                 UnmaterializedUnloadedBlobDirEntry>) {
          return makeImmediateFutureWith([mode = arg.getInitialMode()]() {
            return treeEntryTypeFromMode(mode).value();
          });
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return TreeEntryType::TREE;
        } else if constexpr (std::is_same_v<T, TreeEntry>) {
          return arg.getType();
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<BlobMetadata> VirtualInode::getBlobMetadata(
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  return std::visit(
      [path, objectStore, &fetchContext](
          auto&& arg) mutable -> ImmediateFuture<BlobMetadata> {
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

ImmediateFuture<EntryAttributes> VirtualInode::getEntryAttributes(
    EntryAttributeFlags requestedAttributes,
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  std::optional<folly::Try<Hash20>> sha1;
  std::optional<folly::Try<uint64_t>> size;
  std::optional<folly::Try<TreeEntryType>> type;
  // For non regular files we return errors for hashes and sizes.
  // We intentionally want to refuse to compute the SHA1 of symlinks.
  switch (getDtype()) {
    case dtype_t::Dir:
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SHA1)) {
        sha1 = folly::Try<Hash20>{PathError{EISDIR, path}};
      }
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SIZE)) {
        size = folly::Try<uint64_t>{PathError{EISDIR, path}};
      }
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_TYPE)) {
        type = folly::Try<TreeEntryType>{TreeEntryType::TREE};
      }
      return EntryAttributes{std::move(sha1), std::move(size), std::move(type)};
    case dtype_t::Symlink:
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SHA1)) {
        sha1 = folly::Try<Hash20>{PathError(EINVAL, path, "file is a symlink")};
      }
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SIZE)) {
        size =
            folly::Try<uint64_t>{PathError(EINVAL, path, "file is a symlink")};
      }
      if (requestedAttributes.contains(ENTRY_ATTRIBUTE_TYPE)) {
        type = folly::Try<TreeEntryType>{TreeEntryType::SYMLINK};
      }
      return EntryAttributes{std::move(sha1), std::move(size), std::move(type)};
    case dtype_t::Regular:
      break;
    default:
      return makeImmediateFuture<EntryAttributes>(
          PathError(EINVAL, path, "variant is of unhandled type"));
  }
  // This is now guaranteed to be a dtype_t::Regular file. This
  // means there's no need for a Tree case, as Trees are always
  // directories. It's included to check that the visitor here is
  // exhaustive.
  auto entryTypeFuture = ImmediateFuture<TreeEntryType>{
      PathError{EINVAL, path, "type not requested"}};
  if (requestedAttributes.contains(ENTRY_ATTRIBUTE_TYPE)) {
    entryTypeFuture = getTreeEntryType(path, fetchContext);
  }
  auto blobMetadataFuture = ImmediateFuture<BlobMetadata>{
      PathError{EINVAL, path, "neither sha1 nor size requested"}};
  // sha1 and size come together so, there isn't much point of splitting them up
  if (requestedAttributes.containsAnyOf(
          ENTRY_ATTRIBUTE_SIZE | ENTRY_ATTRIBUTE_SHA1)) {
    blobMetadataFuture = getBlobMetadata(path, objectStore, fetchContext);
  }

  return collectAll(std::move(entryTypeFuture), std::move(blobMetadataFuture))
      .thenValue(
          [requestedAttributes](
              std::tuple<folly::Try<TreeEntryType>, folly::Try<BlobMetadata>>
                  rawAttributeData) mutable -> EntryAttributes {
            std::optional<folly::Try<Hash20>> sha1;
            std::optional<folly::Try<uint64_t>> size;
            std::optional<folly::Try<TreeEntryType>> type;
            if (requestedAttributes.contains(ENTRY_ATTRIBUTE_TYPE)) {
              type = std::move(
                  std::get<folly::Try<TreeEntryType>>(rawAttributeData));
            }
            auto& blobMetadata =
                std::get<folly::Try<BlobMetadata>>(rawAttributeData);

            if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SHA1)) {
              sha1 = blobMetadata.hasException()
                  ? folly::Try<Hash20>(blobMetadata.exception())
                  : folly::Try<Hash20>(blobMetadata.value().sha1);
            }
            if (requestedAttributes.contains(ENTRY_ATTRIBUTE_SIZE)) {
              size = blobMetadata.hasException()
                  ? folly::Try<uint64_t>(blobMetadata.exception())
                  : folly::Try<uint64_t>(blobMetadata.value().size);
            }
            return EntryAttributes{
                std::move(sha1), std::move(size), std::move(type)};
          });
}

// Returns a subset of `struct stat` required by
// EdenServiceHandler::semifuture_getFileInformation()
ImmediateFuture<struct stat> VirtualInode::stat(
    // TODO: can lastCheckoutTime be fetched from some global edenMount()?
    //
    // VirtualInode is used to traverse the tree. However, the global
    // renameLock is NOT held during these traversals, so we're not protected
    // from nodes/trees being moved around during the traversal.
    //
    // It's inconvenient to pass the lastCheckoutTime in from the caller, but we
    // got to this particular location in the mount by starting at a particular
    // root node with that checkout time. Because we don't hold the rename lock,
    // it's not clear if the current global edenMount object's lastCheckoutTime
    // is any more or less correct than the passed in lastCheckoutTime. It's
    // _probably_ safer to use the older one, as that represents what the state
    // of the repository WAS when the traversal started. If we queried the
    // global eden mount here for the lastCheckoutTime, we may get a time in the
    // future when one of our parents changed, and we may be mis-reporting the
    // state of the tree.
    //
    // In short: there's a potential race condition here that may cause
    // mis-reporting.
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

namespace {
/**
 * Helper function for getChildren when the current node is a Tree.
 */
std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>
getChildrenHelper(
    const TreePtr& tree,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) {
  std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>> result{};
  result.reserve(tree->size());

  for (auto& child : *tree) {
    const auto* treeEntry = &child.second;
    if (treeEntry->isTree()) {
      result.push_back(std::make_pair(
          child.first,
          objectStore->getTree(treeEntry->getHash(), fetchContext)
              .thenValue([mode = modeFromTreeEntryType(treeEntry->getType())](
                             TreePtr tree) {
                return VirtualInode{std::move(tree), mode};
              })));
    } else {
      // This is a file, return the TreeEntry for it
      result.push_back(std::make_pair(
          child.first, ImmediateFuture{VirtualInode{*treeEntry}}));
    }
  }

  return result;
}
} // namespace

folly::Try<std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>
VirtualInode::getChildren(
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) {
  if (!isDirectory()) {
    return folly::Try<
        std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>(
        PathError(ENOTDIR, path));
  }
  return std::visit(
      [&](auto&& arg)
          -> folly::Try<std::vector<
              std::pair<PathComponent, ImmediateFuture<VirtualInode>>>> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return folly::Try<std::vector<
              std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>{
              arg.asTreePtr()->getChildren(fetchContext, false)};
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return folly::Try<std::vector<
              std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>{
              getChildrenHelper(arg, objectStore, fetchContext)};
        } else if constexpr (
            std::is_same_v<T, UnmaterializedUnloadedBlobDirEntry> ||
            std::is_same_v<T, TreeEntry>) {
          // These represent files in VirtualInode, and can't be
          // descended
          return folly::Try<std::vector<
              std::pair<PathComponent, ImmediateFuture<VirtualInode>>>>{
              PathError(ENOTDIR, path, "variant is of unhandled type")};
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

ImmediateFuture<
    std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>
VirtualInode::getChildrenAttributes(
    EntryAttributeFlags requestedAttributes,
    RelativePath path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) {
  auto children = this->getChildren(path.piece(), objectStore, fetchContext);

  if (children.hasException()) {
    return ImmediateFuture<
        std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>{
        children.exception()};
  }

  std::vector<PathComponent> names{};
  std::vector<ImmediateFuture<EntryAttributes>> attributesFutures{};

  names.reserve(children.value().size());
  attributesFutures.reserve(children.value().size());

  for (auto& nameAndvirtualInode : children.value()) {
    names.push_back(nameAndvirtualInode.first);
    attributesFutures.push_back(
        std::move(nameAndvirtualInode.second)
            .thenValue([requestedAttributes,
                        subPath = path + nameAndvirtualInode.first,
                        objectStore,
                        &fetchContext](VirtualInode virtualInode) {
              return virtualInode.getEntryAttributes(
                  requestedAttributes, subPath, objectStore, fetchContext);
            }));
  }
  return collectAll(std::move(attributesFutures))
      .thenValue(
          [names = std::move(names)](
              std::vector<folly::Try<EntryAttributes>> attributes) mutable {
            std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>
                zippedResult{};
            zippedResult.reserve(attributes.size());
            XDCHECK_EQ(attributes.size(), names.size())
                << "Missing/too many attributes for the names.";
            for (uint32_t i = 0; i < attributes.size(); ++i) {
              zippedResult.push_back(std::make_pair(
                  std::move(names.at(i)), std::move(attributes.at(i))));
            }
            return zippedResult;
          });
}

namespace {
/**
 * Helper function for getOrFindChild when the current node is a Tree.
 */
ImmediateFuture<VirtualInode> getOrFindChildHelper(
    TreePtr tree,
    PathComponentPiece childName,
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) {
  // Lookup the next child
  const auto it = tree->find(childName);
  if (it == tree->cend()) {
    // Note that the path printed below is the requested path that is being
    // walked, childName may appear anywhere in the path.
    XLOG(DBG7) << "attempted to find non-existent TreeEntry \"" << childName
               << "\" in " << path;
    return makeImmediateFuture<VirtualInode>(
        std::system_error(ENOENT, std::generic_category()));
  }

  // Always descend if the treeEntry is a Tree
  const auto* treeEntry = &it->second;
  if (treeEntry->isTree()) {
    return objectStore->getTree(treeEntry->getHash(), fetchContext)
        .thenValue(
            [mode = modeFromTreeEntryType(treeEntry->getType())](TreePtr tree) {
              return VirtualInode{std::move(tree), mode};
            });
  } else {
    // This is a file, return the TreeEntry for it
    return ImmediateFuture{VirtualInode{*treeEntry}};
  }
}
} // namespace

ImmediateFuture<VirtualInode> VirtualInode::getOrFindChild(
    PathComponentPiece childName,
    RelativePathPiece path,
    ObjectStore* objectStore,
    ObjectFetchContext& fetchContext) const {
  if (!isDirectory()) {
    return makeImmediateFuture<VirtualInode>(PathError(ENOTDIR, path));
  }
  return std::visit(
      [childName, path, objectStore, &fetchContext](
          auto&& arg) -> ImmediateFuture<VirtualInode> {
        using T = std::decay_t<decltype(arg)>;
        if constexpr (std::is_same_v<T, InodePtr>) {
          return arg.asTreePtr()->getOrFindChild(
              childName, fetchContext, false);
        } else if constexpr (std::is_same_v<T, TreePtr>) {
          return getOrFindChildHelper(
              arg, childName, path, objectStore, fetchContext);
        } else if constexpr (
            std::is_same_v<T, UnmaterializedUnloadedBlobDirEntry> ||
            std::is_same_v<T, TreeEntry>) {
          // These represent files in VirtualInode, and can't be descended
          return makeImmediateFuture<VirtualInode>(
              PathError(ENOTDIR, path, "variant is of unhandled type"));
        } else {
          static_assert(always_false_v<T>, "non-exhaustive visitor!");
        }
      },
      variant_);
}

} // namespace facebook::eden
