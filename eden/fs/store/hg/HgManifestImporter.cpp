/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "HgManifestImporter.h"

#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>

#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/store/LocalStore.h"

using folly::ByteRange;
using folly::io::Appender;
using folly::IOBuf;
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
  Hash record(LocalStore* store);

 private:
  // The full path from the root of this repository
  RelativePath path_;

  // LocalStore currently requires that all data be stored in git tree format.
  GitTreeSerializer serializer_;
  unsigned int numPaths_{0};
  std::vector<TreeEntry> entries_;
};

HgManifestImporter::PartialTree::PartialTree(RelativePathPiece path)
    : path_(std::move(path)) {}

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

Hash HgManifestImporter::PartialTree::record(LocalStore* store) {
  auto tree = Tree(std::move(entries_));
  auto hash = store->putTree(&tree);

  VLOG(6) << "record tree: '" << path_ << "' --> " << hash.toString() << " ("
          << numPaths_ << " paths)";

  return hash;
}

HgManifestImporter::HgManifestImporter(LocalStore* store) : store_(store) {
  // Push the root directory onto the stack
  dirStack_.emplace_back(RelativePath(""));
}

HgManifestImporter::~HgManifestImporter() {}

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
        VLOG(5) << "push '" << iter.piece() << "'  # '" << dirname << "'";
        dirStack_.emplace_back(iter.piece());
        ++iter;
      }
      dirStack_.back().addEntry(std::move(entry));
      break;
    }

    // None of the checks above passed, so the current entry must be a parent
    // of the current directory.  Record the current directory, then pop it off
    // the stack.
    VLOG(5) << "pop '" << dirStack_.back().getPath() << "' --> '"
            << (dirStack_.end() - 2)->getPath() << "'  # '" << dirname << "'";
    popAndRecordCurrentDir();
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
    VLOG(5) << "final pop '" << dirStack_.back().getPath() << "'";
    popAndRecordCurrentDir();
  }

  auto rootHash = dirStack_.back().record(store_);
  dirStack_.pop_back();
  CHECK(dirStack_.empty());
  return rootHash;
}

void HgManifestImporter::popAndRecordCurrentDir() {
  PathComponent entryName = dirStack_.back().getPath().basename().copy();

  auto dirHash = dirStack_.back().record(store_);
  dirStack_.pop_back();
  DCHECK(!dirStack_.empty());

  uint8_t ownerPermissions = 0111;
  TreeEntry dirEntry(
      dirHash, entryName.stringPiece(), FileType::DIRECTORY, ownerPermissions);
  dirStack_.back().addEntry(std::move(dirEntry));
}
}
} // facebook::eden
