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
#include "cdatapack/cdatapack.h"
}

#include <ctime>
#include <memory>
#include <string>
#include <unordered_set>
#include <vector>

#include "cstore/datastore.h"
#include "cstore/key.h"
#include "clib/portability/portability.h"

const clock_t PACK_REFRESH_RATE = 0.1 * CLOCKS_PER_SEC;

class DatapackStore;
class DatapackStoreKeyIterator : public KeyIterator {
  private:
    DatapackStore &_store;
    KeyIterator &_missing;

  public:
    DatapackStoreKeyIterator(DatapackStore &store, KeyIterator &missing) :
      _store(store),
      _missing(missing) {}

    Key *next() override;
};

/* Manages access to a directory of datapack files. */
class DatapackStore : public DataStore {
  private:
    std::string _path;
    clock_t _lastRefresh;

    std::unordered_set<std::string> _packPaths;

    datapack_handle_t *addPack(const std::string &path);

    std::vector<datapack_handle_t*> refresh();
  public:
    std::vector<datapack_handle_t*> _packs;

    DatapackStore(const std::string &path);

    ~DatapackStore();

    DeltaChainIterator getDeltaChain(const Key &key);

    std::shared_ptr<KeyIterator> getMissing(KeyIterator &missing);

    std::shared_ptr<DeltaChain> getDeltaChainRaw(const Key &key);

    bool contains(const Key &key);

    void markForRefresh();
};

#endif // FBHGEXT_DATAPACKSTORE_H
