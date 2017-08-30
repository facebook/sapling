// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// datastore.h - c++ declarations for a data store
// no-check-code

#ifndef FBHGEXT_DATASTORE_H
#define FBHGEXT_DATASTORE_H

extern "C" {
#include "cdatapack/cdatapack.h"
}

#include <memory>
#include <vector>

#include "cstore/key.h"
#include "clib/portability/portability.h"

class DeltaChainLink {
  private:
    const char *_filename, *_deltabasefilename;
    const uint8_t *_node, *_deltabasenode, *_delta;
    uint16_t _filenamesz;
    uint64_t _deltasz;

  public:
    DeltaChainLink(delta_chain_link_t *link) {
      if (link) {
        _filename = link->filename;
        _deltabasefilename = link->filename;
        _node = link->node;
        _deltabasenode = link->deltabase_node;
        _delta = link->delta;
        _filenamesz = link->filename_sz;
        _deltasz = link->delta_sz;
      } else {
        _filename = NULL;
        _deltabasefilename = NULL;
        _node = NULL;
        _deltabasenode = NULL;
        _delta = NULL;
        _filenamesz = 0;
        _deltasz = 0;
      }
    }

    const char* filename() {
      return _filename;
    }

    const char* deltabasefilename() {
      return _deltabasefilename;
    }

    const uint8_t* node() {
      return _node;
    }

    const uint8_t* deltabasenode() {
      return _deltabasenode;
    }

    const uint8_t* delta() {
      return _delta;
    }

    uint16_t filenamesz() {
      return _filenamesz;
    }

    uint64_t deltasz() {
      return _deltasz;
    }

    bool isdone() {
      return (_filename == NULL);
    }
};

/*
 * This class takes ownership of a delta chain
 */
class DeltaChain {
  private:
    //C DeltaChain
    delta_chain_t _chain;

  public:
    //The constructor does a shallow copy of the delta chain and since the
    //ownership is taken by this class it is responsible for memory management
    DeltaChain(delta_chain_t chain) : _chain(chain) {}

    DeltaChain(get_delta_chain_code_t error) {
      _chain = COMPOUND_LITERAL(delta_chain_t) { GET_DELTA_CHAIN_NOT_FOUND };
    }

    ~DeltaChain() {
      freedeltachain(_chain);
    }

    const DeltaChainLink getlink(const size_t idx) {
      return DeltaChainLink(&(_chain.delta_chain_links[idx]));
    }

    size_t linkcount() {
      return _chain.links_count;
    }

    get_delta_chain_code_t code() {
      return _chain.code;
    }

};

class DeltaChainIterator {
  private:
    size_t _index;
  protected:
    std::vector< std::shared_ptr<DeltaChain> > _chains;
    DeltaChainIterator() :
      _index(0) {}
    virtual std::shared_ptr<DeltaChain> getNextChain(const Key &key) {
      return std::make_shared<DeltaChain>(GET_DELTA_CHAIN_NOT_FOUND);
    }
  public:
    DeltaChainIterator(std::shared_ptr<DeltaChain> chain) :
      _index(0) {
      _chains.push_back(chain);
    }

    DeltaChainLink next() {
      std::shared_ptr<DeltaChain> chain = _chains.back();

      if (_index >= chain->linkcount()) {
        // If we're not at the end, and we have a callback to fetch more, do the
        // fetch.
        bool refreshed = false;
        if (chain->linkcount() > 0) {
          DeltaChainLink result = chain->getlink(_index - 1);

          const uint8_t *deltabasenode = result.deltabasenode();
          if (memcmp(deltabasenode, NULLID, BIN_NODE_SIZE) != 0) {
            Key key(result.filename(), result.filenamesz(),
                    (const char*)deltabasenode, BIN_NODE_SIZE);

            std::shared_ptr<DeltaChain> newChain = this->getNextChain(key);
            if (newChain->code() == GET_DELTA_CHAIN_OK) {
              // Do not free the old chain, since the iterator consumer may
              // still be holding references to it.
              _chains.push_back(newChain);
              chain = _chains.back();
              _index = 0;
              refreshed = true;
            }
          }
        }

        if (!refreshed) {
          return DeltaChainLink(NULL);
        }
      }

      DeltaChainLink result = chain->getlink(_index);
      _index++;

      return result;
    }
};

class DataStore {
  protected:
    DataStore() {}

  public:
    virtual ~DataStore() {}

    virtual DeltaChainIterator getDeltaChain(const Key &key) = 0;

    virtual std::shared_ptr<DeltaChain> getDeltaChainRaw(const Key &key) = 0;

    virtual std::shared_ptr<KeyIterator> getMissing(KeyIterator &missing) = 0;

    virtual bool contains(const Key &key) = 0;

    virtual void markForRefresh() = 0;
};

#endif // FBHGEXT_DATASTORE_H
