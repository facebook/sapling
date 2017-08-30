// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// uniondatapackstore.h - c++ declarations for a union datapack store
// no-check-code

#ifndef FBHGEXT_CSTORE_UNIONDATAPACKSTORE_H
#define FBHGEXT_CSTORE_UNIONDATAPACKSTORE_H

#include <cstring>
#include <stdexcept>
#include <vector>

extern "C" {
#include "cdatapack/cdatapack.h"
}

#include "cstore/key.h"
#include "cstore/datapackstore.h"
#include "cstore/store.h"

class UnionDatapackStore;
class UnionDatapackStoreKeyIterator : public KeyIterator {
  private:
    UnionDatapackStore &_store;
    KeyIterator &_missing;

  public:
    UnionDatapackStoreKeyIterator(UnionDatapackStore &store, KeyIterator &missing) :
      _store(store),
      _missing(missing) {}

    Key *next() override;
};

class UnionDeltaChainIterator: public DeltaChainIterator {
  private:
    UnionDatapackStore &_store;
  protected:
    std::shared_ptr<DeltaChain> getNextChain(const Key &key) override;

  public:
    UnionDeltaChainIterator(UnionDatapackStore &store, const Key &key) :
      DeltaChainIterator(),
      _store(store) {
      _chains.push_back(this->getNextChain(key));
    }
};

class UnionDatapackStore : public Store {
  public:
    std::vector<DataStore*> _stores;

    UnionDatapackStore(std::vector<DataStore*> stores);

    ~UnionDatapackStore() override;

    ConstantStringRef get(const Key &key) override;

    UnionDeltaChainIterator getDeltaChain(const Key &key);

    bool contains(const Key &key);

    UnionDatapackStoreKeyIterator getMissing(KeyIterator &missing);

    void markForRefresh();
};

#endif // FBHGEXT_CSTORE_UNIONDATAPACKSTORE_H
