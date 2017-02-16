// uniondatapackstore.h - c++ declarations for a union datapack store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef UNIONDATAPACKSTORE_H
#define UNIONDATAPACKSTORE_H

extern "C" {
#include "../cdatapack/cdatapack.h"
}

#include "key.h"
#include "datapackstore.h"

#include <cstring>
#include <vector>
#include <stdexcept>

class UnionDatapackStore;
class UnionDatapackStoreKeyIterator : public KeyIterator {
  private:
    UnionDatapackStore &_store;
    KeyIterator &_missing;

  public:
    UnionDatapackStoreKeyIterator(UnionDatapackStore &store, KeyIterator &missing) :
      _store(store),
      _missing(missing) {}

    Key *next();
};

class UnionDatapackStore {
  public:
    std::vector<DatapackStore*> _stores;

    UnionDatapackStore(std::vector<DatapackStore*> stores);

    ~UnionDatapackStore();

    bool contains(const Key &key);

    UnionDatapackStoreKeyIterator getMissing(KeyIterator &missing);
};

#endif //UNIONDATAPACKSTORE_H
