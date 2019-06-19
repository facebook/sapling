/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/FakeTreeBuilder.h"

#include <folly/Conv.h>
#include <stdexcept>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/StoredObject.h"

using std::make_unique;
using std::string;

namespace facebook {
namespace eden {

FakeTreeBuilder::FakeTreeBuilder() {}

/**
 * Create a clone of this FakeTreeBuilder.
 *
 * The clone has the same path data (stored in root_) but is not finalized,
 * regardless of whether the original FakeTreeBuilder was finalized or not.
 */
FakeTreeBuilder::FakeTreeBuilder(ExplicitClone, const FakeTreeBuilder* orig)
    : root_{CLONE, orig->root_} {}

FakeTreeBuilder FakeTreeBuilder::clone() const {
  return FakeTreeBuilder{CLONE, this};
}

void FakeTreeBuilder::setFiles(
    const std::initializer_list<FileInfo>& fileArgs) {
  for (const auto& arg : fileArgs) {
    setFileImpl(
        arg.path,
        folly::ByteRange{folly::StringPiece{arg.contents}},
        false,
        arg.executable ? TreeEntryType::EXECUTABLE_FILE
                       : TreeEntryType::REGULAR_FILE);
  }
}

void FakeTreeBuilder::mkdir(RelativePathPiece path) {
  // Use getDirEntry() to create a directory at this location if one
  // does not already exist.
  getDirEntry(path, true);
}

void FakeTreeBuilder::setFileImpl(
    RelativePathPiece path,
    folly::ByteRange contents,
    bool replace,
    TreeEntryType type) {
  CHECK(!finalizedRoot_);

  auto dir = getDirEntry(path.dirname(), true);
  auto name = path.basename();

  auto info = EntryInfo{type};
  info.contents = folly::StringPiece{contents}.str();

  if (replace) {
    auto iter = dir->entries->find(name);
    if (iter == dir->entries->end()) {
      throw std::runtime_error(folly::to<string>(
          "while building fake tree: expected to replace entry at ",
          path,
          " but no entry present with this name"));
    }
    iter->second = std::move(info);
  } else {
    auto ret = dir->entries->emplace(name, std::move(info));
    if (!ret.second) {
      throw std::runtime_error(folly::to<string>(
          "while building fake tree: an entry already exists at ", path));
    }
  }
}

void FakeTreeBuilder::removeFile(
    RelativePathPiece path,
    bool removeEmptyParents) {
  CHECK(!finalizedRoot_);

  auto parentPath = path.dirname();
  auto dir = getDirEntry(parentPath, false);
  auto name = path.basename();
  auto iter = dir->entries->find(name);
  if (iter == dir->entries->end()) {
    throw std::runtime_error(folly::to<string>(
        "while building fake tree: expected to remove entry at ",
        path,
        " but no entry present with this name"));
  }
  dir->entries->erase(iter);

  if (removeEmptyParents && dir->entries->empty()) {
    removeFile(parentPath, true);
  }
}

void FakeTreeBuilder::setReady(RelativePathPiece path) {
  CHECK(finalizedRoot_) << "call finalize before setReady";

  if (path.empty()) {
    finalizedRoot_->setReady();
    return;
  }

  auto* parent = getStoredTree(path.dirname());
  const auto& entry = parent->get().getEntryAt(path.basename());
  if (entry.isTree()) {
    store_->getStoredTree(entry.getHash())->setReady();
  } else {
    store_->getStoredBlob(entry.getHash())->setReady();
  }
}

void FakeTreeBuilder::setAllReady() {
  CHECK(finalizedRoot_);
  setAllReadyUnderTree(finalizedRoot_);
}

void FakeTreeBuilder::setAllReadyUnderTree(RelativePathPiece path) {
  auto tree = getStoredTree(path);
  return setAllReadyUnderTree(tree);
}

void FakeTreeBuilder::setAllReadyUnderTree(StoredTree* tree) {
  tree->setReady();
  for (const auto& entry : tree->get().getTreeEntries()) {
    if (entry.isTree()) {
      auto* child = store_->getStoredTree(entry.getHash());
      setAllReadyUnderTree(child);
    } else {
      auto* child = store_->getStoredBlob(entry.getHash());
      child->setReady();
    }
  }
}

void FakeTreeBuilder::triggerError(
    RelativePathPiece path,
    folly::exception_wrapper ew) {
  CHECK(finalizedRoot_);

  if (path.empty()) {
    finalizedRoot_->triggerError(std::move(ew));
    return;
  }

  auto* parent = getStoredTree(path.dirname());
  const auto& entry = parent->get().getEntryAt(path.basename());
  if (entry.isTree()) {
    store_->getStoredTree(entry.getHash())->triggerError(std::move(ew));
  } else {
    store_->getStoredBlob(entry.getHash())->triggerError(std::move(ew));
  }
}

StoredTree* FakeTreeBuilder::finalize(
    std::shared_ptr<FakeBackingStore> store,
    bool setReady) {
  CHECK(!finalizedRoot_);
  CHECK(!store_);
  store_ = std::move(store);

  finalizedRoot_ = root_.finalizeTree(this, setReady);
  return finalizedRoot_;
}

StoredTree* FakeTreeBuilder::getRoot() const {
  CHECK(finalizedRoot_);
  return finalizedRoot_;
}

FakeTreeBuilder::EntryInfo* FakeTreeBuilder::getEntry(RelativePathPiece path) {
  if (path.empty()) {
    return &root_;
  }

  auto* parent = getDirEntry(path.dirname(), false);
  auto iter = parent->entries->find(path.basename());
  if (iter == parent->entries->end()) {
    throw std::runtime_error(
        folly::to<string>("tried to look up non-existent entry ", path));
  }
  return &iter->second;
}

FakeTreeBuilder::EntryInfo* FakeTreeBuilder::getDirEntry(
    RelativePathPiece path,
    bool create) {
  EntryInfo* parent = &root_;

  for (auto name : path.components()) {
    auto iter = parent->entries->find(name);
    if (iter == parent->entries->end()) {
      if (!create) {
        throw std::runtime_error(folly::to<string>(
            "tried to look up non-existent directory ", path));
      }
      auto ret = parent->entries->emplace(name, EntryInfo{TreeEntryType::TREE});
      CHECK(ret.second);
      parent = &ret.first->second;
    } else {
      parent = &iter->second;
      if (parent->type != TreeEntryType::TREE) {
        throw std::runtime_error(folly::to<string>(
            "tried to look up directory ",
            path,
            " but ",
            name,
            " is not a directory"));
      }
    }
  }

  return parent;
}

StoredTree* FakeTreeBuilder::getStoredTree(RelativePathPiece path) {
  CHECK(finalizedRoot_);

  StoredTree* current = finalizedRoot_;
  for (auto name : path.components()) {
    const auto& entry = current->get().getEntryAt(name);
    if (!entry.isTree()) {
      throw std::runtime_error(folly::to<string>(
          "tried to look up stored tree ",
          path,
          " but ",
          name,
          " is not a tree"));
    }

    current = store_->getStoredTree(entry.getHash());
  }

  return current;
}

StoredBlob* FakeTreeBuilder::getStoredBlob(RelativePathPiece path) {
  auto* parent = getStoredTree(path.dirname());
  const auto& entry = parent->get().getEntryAt(path.basename());
  if (entry.isTree()) {
    throw std::runtime_error(folly::to<string>(
        "tried to look up stored blob at ",
        path,
        " but it is a tree rather than a blob"));
  }
  return store_->getStoredBlob(entry.getHash());
}

FakeTreeBuilder::EntryInfo::EntryInfo(TreeEntryType fileType) : type(fileType) {
  if (type == TreeEntryType::TREE) {
    entries = make_unique<PathMap<EntryInfo>>();
  }
}

FakeTreeBuilder::EntryInfo::EntryInfo(ExplicitClone, const EntryInfo& orig)
    : type(orig.type), contents(orig.contents) {
  if (orig.entries) {
    entries = make_unique<PathMap<EntryInfo>>();
    for (const auto& e : *orig.entries) {
      auto ret = entries->emplace(e.first, EntryInfo{CLONE, e.second});
      CHECK(ret.second) << "failed to insert " << e.first;
    }
  }
}

StoredTree* FakeTreeBuilder::EntryInfo::finalizeTree(
    FakeTreeBuilder* builder,
    bool setReady) const {
  CHECK(type == TreeEntryType::TREE);

  std::vector<TreeEntry> treeEntries;
  for (const auto& e : *entries) {
    const auto& entryInfo = e.second;
    Hash hash;
    if (entryInfo.type == TreeEntryType::TREE) {
      auto* storedTree = entryInfo.finalizeTree(builder, setReady);
      hash = storedTree->get().getHash();
    } else {
      auto* storedBlob = entryInfo.finalizeBlob(builder, setReady);
      hash = storedBlob->get().getHash();
    }
    treeEntries.emplace_back(hash, e.first.stringPiece(), entryInfo.type);
  }

  auto* storedTree = builder->store_->maybePutTree(treeEntries).first;
  if (setReady) {
    storedTree->setReady();
  }
  return storedTree;
}

StoredBlob* FakeTreeBuilder::EntryInfo::finalizeBlob(
    FakeTreeBuilder* builder,
    bool setReady) const {
  CHECK(type != TreeEntryType::TREE);
  auto* storedBlob = builder->store_->maybePutBlob(contents).first;
  if (setReady) {
    storedBlob->setReady();
  }
  return storedBlob;
}
} // namespace eden
} // namespace facebook
