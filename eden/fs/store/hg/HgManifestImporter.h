/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <vector>

#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class Hash;
class TreeEntry;

/*
 * HgManifestImporter maintains state needed to process an
 * HG manifest and create Tree objects from it.
 */
class HgManifestImporter {
 public:
  explicit HgManifestImporter(
      LocalStore* store,
      LocalStore::WriteBatch* writeBatch);
  virtual ~HgManifestImporter();

  /**
   * processEntry() should be called for each manifest entry.
   *
   * This should be called in the order they are received from mercurial.
   * (mercurial keeps the entries in sorted order.)
   */
  void processEntry(RelativePathPiece dirname, TreeEntry&& entry);

  /**
   * finish() should be called once processEntry() has been called for
   * all entries in the manifest.
   *
   * It returns the hash identifying the root Tree.
   */
  Hash finish();

  LocalStore* getLocalStore() const {
    return store_;
  }

 private:
  class PartialTree;

  // Forbidden copy constructor and assignment operator
  HgManifestImporter(const HgManifestImporter&) = delete;
  HgManifestImporter& operator=(const HgManifestImporter&) = delete;

  void popCurrentDir();

  LocalStore* store_{nullptr};
  std::vector<PartialTree> dirStack_;
  LocalStore::WriteBatch* writeBatch_;
};
} // namespace eden
} // namespace facebook
