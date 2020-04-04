/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/store/WinStore.h"
#include <folly/Format.h>
#include <folly/logging/xlog.h>
#include <cstring>
#include <shared_mutex>
#include "eden/fs/inodes/win/EdenMount.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/win/utils/StringConv.h"

using namespace std;
using namespace folly;

namespace facebook {
namespace eden {

WinStore::WinStore(const EdenMount& mount) : mount_{mount} {
  XLOGF(
      INFO,
      "Creating WinStore mount(0x{:x}) root {} WinStore (0x{:x}))",
      reinterpret_cast<uintptr_t>(&mount),
      mount.getPath(),
      reinterpret_cast<uintptr_t>(this));
}
WinStore ::~WinStore() {}

shared_ptr<const Tree> WinStore::getTree(
    const RelativePathPiece& relPath) const {
  auto tree = getMount().getRootTree().get();

  for (auto piece : relPath.components()) {
    auto entry = tree->getEntryPtr(piece);
    if (entry != nullptr && entry->isTree()) {
      tree =
          getMount()
              .getObjectStore()
              ->getTree(entry->getHash(), ObjectFetchContext::getNullContext())
              .get();
    } else {
      return nullptr;
    }
  }
  return tree;
}

shared_ptr<const Tree> WinStore::getTree(const std::wstring_view path) const {
  std::string edenPath = winToEdenPath(path);
  RelativePathPiece relPath{edenPath};
  return getTree(relPath);
}

bool WinStore::getAllEntries(
    const std::wstring_view path,
    vector<FileMetadata>& entryList) const {
  shared_ptr<const Tree> tree = getTree(path);

  if (!tree) {
    return false;
  }

  const std::vector<TreeEntry>& treeEntries = tree->getTreeEntries();
  vector<Future<pair<uint64_t, size_t>>> futures;
  for (size_t i = 0; i < treeEntries.size(); i++) {
    size_t fileSize = 0;
    if (!treeEntries[i].isTree()) {
      const std::optional<uint64_t>& size = treeEntries[i].getSize();
      if (size.has_value()) {
        fileSize = size.value();
      } else {
        futures.emplace_back(getMount()
                                 .getObjectStore()
                                 ->getBlobSize(
                                     treeEntries[i].getHash(),
                                     ObjectFetchContext::getNullContext())
                                 .thenValue([index = i](auto size) {
                                   return make_pair(size, index);
                                 }));
        continue;
      }
    }

    entryList.emplace_back(
        std::move(
            edenToWinName(treeEntries[i].getName().value().toStdString())),
        treeEntries[i].isTree(),
        fileSize);
  }

  auto results = folly::collectAll(std::move(futures)).get();
  for (auto& result : results) {
    //
    // If we are here it's for a file, so the second argument will be false.
    //
    entryList.emplace_back(
        std::move(edenToWinName(
            treeEntries[result->second].getName().value().toStdString())),
        false,
        result->first);
  }

  return true;
}

bool WinStore::getFileMetadata(
    const std::wstring_view path,
    FileMetadata& fileMetadata) const {
  RelativePath relPath{wideCharToEdenRelativePath(path)};
  shared_ptr<const Tree> tree = getTree(relPath.dirname());
  if (!tree) {
    return false;
  }

  auto entry = tree->getEntryPtr(relPath.basename());
  if (entry) {
    fileMetadata.name = edenToWinName(entry->getName().value().toStdString());
    fileMetadata.isDirectory = entry->isTree();
    fileMetadata.hash = entry->getHash();

    if (!fileMetadata.isDirectory) {
      const std::optional<uint64_t>& size = entry->getSize();
      if (size.has_value()) {
        fileMetadata.size = size.value();
      } else {
        auto size =
            getMount()
                .getObjectStore()
                ->getBlobSize(
                    entry->getHash(), ObjectFetchContext::getNullContext())
                .get();
        fileMetadata.size = size;
      }
    }
    return true;
  }
  return false;
}

bool WinStore::checkFileName(const std::wstring_view path) const {
  RelativePath relPath{wideCharToEdenRelativePath(path)};
  shared_ptr<const Tree> tree = getTree(relPath.dirname());
  if (!tree) {
    return false;
  }

  auto entry = tree->getEntryPtr(relPath.basename());
  if (entry) {
    return true;
  }
  return false;
}

std::shared_ptr<const Blob> WinStore::getBlob(
    const std::wstring_view path) const {
  RelativePath relPath{wideCharToEdenRelativePath(path)};
  shared_ptr<const Tree> tree = getTree(relPath.dirname());
  if (!tree) {
    return nullptr;
  }

  auto file = tree->getEntryPtr(relPath.basename());
  if ((file) && (!file->isTree())) {
    return (getMount()
                .getObjectStore()
                ->getBlob(file->getHash(), ObjectFetchContext::getNullContext())
                .get());
  }
  return nullptr;
}

} // namespace eden
} // namespace facebook
