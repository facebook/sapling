// datapackstore.h - c++ declarations for a data pack store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef DATAPACKSTORE_H
#define DATAPACKSTORE_H

extern "C" {
#include "../cdatapack/cdatapack.h"
}

#include <string>
#include <vector>

#include "key.h"

class DeltaChainIterator {
  private:
    delta_chain_t _chain;
    size_t _index;
  public:
    DeltaChainIterator(delta_chain_t chain) :
      _chain(chain),
      _index(0) {
    }

    ~DeltaChainIterator() {
      freedeltachain(_chain);
    }

    delta_chain_link_t *next() {
      if (_index >= _chain.links_count) {
        return NULL;
      }

      delta_chain_link_t *result = &_chain.delta_chain_links[_index];
      _index++;

      return result;
    }

    size_t size() {
      return _chain.links_count;
    }
};

class DatapackStore;
class DatapackStoreKeyIterator : public KeyIterator {
  private:
    DatapackStore &_store;
    KeyIterator &_missing;

  public:
    DatapackStoreKeyIterator(DatapackStore &store, KeyIterator &missing) :
      _store(store),
      _missing(missing) {}

    Key *next();
};

/* Manages access to a directory of datapack files. */
class DatapackStore {
  private:
    std::string _path;

    // The last time we checked the pack directory
    int _lastrefresh;
  public:
    std::vector<datapack_handle_t*> _packs;

    DatapackStore(const std::string &path);

    ~DatapackStore();

    DeltaChainIterator getDeltaChain(const Key &key);

    DatapackStoreKeyIterator getMissing(KeyIterator &missing);

    bool contains(const Key &key);
};

#endif //DATAPACKSTORE_H
