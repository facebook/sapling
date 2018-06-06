// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// datapackstore.h - c++ declarations for a data pack store
// no-check-code

#ifndef FBHGEXT_DATAPACKSTORE_H
#define FBHGEXT_DATAPACKSTORE_H

extern "C" {
#include "lib/cdatapack/cdatapack.h"
}

#include <ctime>
#include <memory>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "hgext/extlib/cstore/datastore.h"
#include "hgext/extlib/cstore/key.h"
#include "lib/clib/portability/portability.h"

const clock_t PACK_REFRESH_RATE = 0.1 * CLOCKS_PER_SEC;

class DatapackStore;
class DatapackStoreKeyIterator : public KeyIterator {
 private:
  DatapackStore& _store;
  KeyIterator& _missing;

 public:
  DatapackStoreKeyIterator(DatapackStore& store, KeyIterator& missing)
      : _store(store), _missing(missing) {}

  Key* next() override;
};

/* Manages access to a directory of datapack files. */
class DatapackStore : public DataStore {
 private:
  std::string path_;
  clock_t lastRefresh_;
  bool removeOnRefresh_;
  std::unordered_map<std::string, std::shared_ptr<datapack_handle_t>> packs_;

  std::shared_ptr<datapack_handle_t> addPack(const std::string& path);
  std::vector<std::shared_ptr<datapack_handle_t>> refresh();

 public:
  ~DatapackStore();
  /** Initialize the store for the specified path.
   * If removeDeadPackFilesOnRefresh is set to true (NOT the default),
   * then the refresh() method can choose to unmap pack files that
   * have been deleted.  Since the DataStore API doesn't provide
   * for propagating ownership out through the DeltaChain and DeltaChain
   * iterator, it is not safe to removeDeadPackFilesOnRefresh if the calling
   * code is keeping longlived references to those values; it is the
   * responsibility of the calling code to ensure that the lifetime is
   * managed correctly as it cannot be enforced automatically without
   * restructing this API.
   */
  explicit DatapackStore(
      const std::string& path,
      bool removeDeadPackFilesOnRefresh = false);

  DeltaChainIterator getDeltaChain(const Key& key) override;

  std::shared_ptr<KeyIterator> getMissing(KeyIterator& missing) override;

  std::shared_ptr<DeltaChain> getDeltaChainRaw(const Key& key) override;

  bool contains(const Key& key) override;

  void markForRefresh() override;
};

#endif // FBHGEXT_DATAPACKSTORE_H
