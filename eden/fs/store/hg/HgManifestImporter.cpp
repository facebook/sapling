/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "HgManifestImporter.h"

#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>

#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/store/LocalStore.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::io::Appender;
using std::string;

namespace facebook {
namespace eden {

/*
 * PartialTree records the in-progress data for a Tree object as we are
 * continuing to receive information about paths inside this directory.
 */
class HgManifestImporter::PartialTree {
 public:
  explicit PartialTree(RelativePathPiece path);

  // Movable but not copiable
  PartialTree(PartialTree&&) noexcept = default;
  PartialTree& operator=(PartialTree&&) noexcept = default;

  const RelativePath& getPath() const {
    return path_;
  }

  void addEntry(TreeEntry&& entry);

  /** move in a computed sub-tree.
   * The tree will be recorded in the store in the second pass of
   * the import, but only if the parent(s) are not stored. */
  void addPartialTree(PartialTree&& tree);

  /** Record this node against the store.
   * May only be called after compute() has been called (this method
   * will check and assert on this). */
  Hash record(LocalStore* store, LocalStore::WriteBatch* batch);

  /** Compute the serialized version of this tree.
   * Records the id and data ready to be stored by a later call
   * to the record() method. */
  Hash compute(LocalStore* store);

 private:
  // The full path from the root of this repository
  RelativePath path_;

  unsigned int numPaths_{0};
  std::vector<TreeEntry> entries_;

  // Serialized data and id that we may need to store;
  // this is the representation of this PartialTree instance.
  Hash id_;
  folly::IOBuf treeData_;
  bool computed_{false};

  // Children that we may need to store
  std::vector<PartialTree> trees_;
};

HgManifestImporter::PartialTree::PartialTree(RelativePathPiece path)
    : path_(std::move(path)) {}

void HgManifestImporter::PartialTree::addPartialTree(PartialTree&& tree) {
  trees_.emplace_back(std::move(tree));
}

void HgManifestImporter::PartialTree::addEntry(TreeEntry&& entry) {
  // Common case should be that we append because we expect the entries
  // to be in the correct sorted order most of the time.
  if (entries_.empty() || entries_.back().getName() < entry.getName()) {
    entries_.emplace_back(std::move(entry));
  } else {
    // The last entry in entries_ sorts after the entry that we wish to
    // insert now.  Let's find the true insertion point.  We use binary
    // search for this rather than a linear backwards scan because some of our
    // directory entries are very large and we may have to go back as many as
    // 100 entries or more to find the correct insertion point.
    auto position = std::lower_bound(
        entries_.begin(),
        entries_.end(),
        entry,
        [](const TreeEntry& a, const TreeEntry& b) {
          return a.getName() < b.getName();
        });
    entries_.emplace(position, std::move(entry));
  }

  ++numPaths_;
}

Hash HgManifestImporter::PartialTree::compute(LocalStore* store) {
  DCHECK(!computed_) << "Can only compute a PartialTree once";
  auto tree = Tree(std::move(entries_));
  std::tie(id_, treeData_) = store->serializeTree(&tree);

  computed_ = true;
  XLOG(DBG6) << "compute tree: '" << path_ << "' --> " << id_.toString() << " ("
             << numPaths_ << " paths)";

  return id_;
}

Hash HgManifestImporter::PartialTree::record(
    LocalStore* store,
    LocalStore::WriteBatch* batch) {
  DCHECK(computed_) << "Must have computed PartialTree prior to recording";
  // If the store already has data on this node, then we don't need to
  // recurse into any of our children; we're done!
  if (store->hasKey(LocalStore::KeySpace::TreeFamily, id_)) {
    return id_;
  }

  // make sure that we try to store each of our children before we try
  // to store this node, so that failure to store one of these prevents
  // us from storing a parent for which we have no children computed.
  for (auto& it : trees_) {
    it.record(store, batch);
  }

  batch->put(LocalStore::KeySpace::TreeFamily, id_, treeData_.coalesce());

  XLOG(DBG6) << "record tree: '" << path_ << "' --> " << id_.toString() << " ("
             << numPaths_ << " paths, " << trees_.size() << " trees)";

  return id_;
}

HgManifestImporter::HgManifestImporter(
    LocalStore* store,
    LocalStore::WriteBatch* writeBatch)
    : store_(store), writeBatch_(writeBatch) {
  // Push the root directory onto the stack
  dirStack_.emplace_back(RelativePath(""));
}

HgManifestImporter::~HgManifestImporter() {
  // Explicitly place the destructor here, otherwise HgImporter.cpp
  // fails to compile because PartialTree is internal to this file.
}

void HgManifestImporter::processEntry(
    RelativePathPiece dirname,
    TreeEntry&& entry) {
  CHECK(!dirStack_.empty());

  // mercurial always maintains the manifest in sorted order,
  // so we can take advantage of this when processing the entries.
  while (true) {
    // If this entry is for the current directory,
    // we can just add the tree entry to the current PartialTree.
    if (dirname == dirStack_.back().getPath()) {
      dirStack_.back().addEntry(std::move(entry));
      break;
    }

    // If this is for a subdirectory of the current directory,
    // we have to push new directories onto the stack.
    auto iter = dirname.findParent(dirStack_.back().getPath());
    auto end = dirname.allPaths().end();
    if (iter != end) {
      ++iter;
      while (iter != end) {
        XLOG(DBG7) << "push '" << iter.piece() << "'  # '" << dirname << "'";
        dirStack_.emplace_back(iter.piece());
        ++iter;
      }
      dirStack_.back().addEntry(std::move(entry));
      break;
    }

    // None of the checks above passed, so the current entry must be a parent
    // of the current directory.  Record the current directory, then pop it off
    // the stack.
    XLOG(DBG7) << "pop '" << dirStack_.back().getPath() << "' --> '"
               << (dirStack_.end() - 2)->getPath() << "'  # '" << dirname
               << "'";
    popCurrentDir();
    CHECK(!dirStack_.empty());
    // Continue around the while loop, now that the current directory
    // is updated.
    continue;
  }
}

Hash HgManifestImporter::finish() {
  CHECK(!dirStack_.empty());

  // The last entry may have been in a deep subdirectory.
  // Pop everything off dirStack_, and record the trees as we go.
  while (dirStack_.size() > 1) {
    XLOG(DBG7) << "final pop '" << dirStack_.back().getPath() << "'";
    popCurrentDir();
  }

  auto rootHash = dirStack_.back().compute(store_);
  dirStack_.back().record(store_, writeBatch_);
  dirStack_.pop_back();
  CHECK(dirStack_.empty());

  writeBatch_->flush();

  return rootHash;
}

void HgManifestImporter::popCurrentDir() {
  PathComponent entryName = dirStack_.back().getPath().basename().copy();

  PartialTree back = std::move(dirStack_.back());
  dirStack_.pop_back();
  DCHECK(!dirStack_.empty());

  auto dirHash = back.compute(store_);

  TreeEntry dirEntry{dirHash, entryName.stringPiece(), TreeEntryType::TREE};
  dirStack_.back().addEntry(std::move(dirEntry));
  dirStack_.back().addPartialTree(std::move(back));
}
} // namespace eden
} // namespace facebook
