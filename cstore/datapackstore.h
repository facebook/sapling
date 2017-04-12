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
#include <ctime>
#include <unordered_set>
#include <vector>

#include "key.h"
#include "../portability/portability.h"

const clock_t PACK_REFRESH_RATE = 0.1 * CLOCKS_PER_SEC;

class DeltaChainIterator {
  private:
    size_t _index;
  protected:
    std::vector<delta_chain_t> _chains;
    DeltaChainIterator() :
      _index(0) {}
    virtual delta_chain_t getNextChain(const Key &key) {
      return COMPOUND_LITERAL(delta_chain_t) { GET_DELTA_CHAIN_NOT_FOUND };
    }
  public:
    DeltaChainIterator(delta_chain_t chain) :
      _index(0) {
      _chains.push_back(chain);
    }

    virtual ~DeltaChainIterator() {
      for(std::vector<delta_chain_t>::iterator it = _chains.begin();
          it != _chains.end();
          it++) {
        freedeltachain(*it);
      }
    }

    delta_chain_link_t *next() {
      delta_chain_t *chain = &_chains.back();
      if (_index >= chain->links_count) {
        // If we're not at the end, and we have a callback to fetch more, do the
        // fetch.
        bool refreshed = false;
        if (chain->links_count > 0) {
          delta_chain_link_t *result = &chain->delta_chain_links[_index - 1];

          const uint8_t *deltabasenode = result->deltabase_node;
          if (memcmp(deltabasenode, NULLID, BIN_NODE_SIZE) != 0) {
            Key key(result->filename, result->filename_sz,
                    (const char*)deltabasenode, BIN_NODE_SIZE);

            delta_chain_t newChain = this->getNextChain(key);
            if (newChain.code == GET_DELTA_CHAIN_OK) {
              // Do not free the old chain, since the iterator consumer may
              // still be holding references to it.
              _chains.push_back(newChain);
              chain = &_chains.back();
              _index = 0;
              refreshed = true;
            } else {
              freedeltachain(newChain);
            }
          }
        }

        if (!refreshed) {
          return NULL;
        }
      }

      delta_chain_link_t *result = &chain->delta_chain_links[_index];
      _index++;

      return result;
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
    clock_t _lastRefresh;

    std::unordered_set<std::string> _packPaths;

    datapack_handle_t *addPack(const std::string &path);

    std::vector<datapack_handle_t*> refresh();
  public:
    std::vector<datapack_handle_t*> _packs;

    DatapackStore(const std::string &path);

    ~DatapackStore();

    DeltaChainIterator getDeltaChain(const Key &key);

    DatapackStoreKeyIterator getMissing(KeyIterator &missing);

    delta_chain_t getDeltaChainRaw(const Key &key);

    bool contains(const Key &key);

    void markForRefresh();
};

#endif //DATAPACKSTORE_H
