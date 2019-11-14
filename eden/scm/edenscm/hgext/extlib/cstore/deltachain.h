/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// deltachain.h - c++ declaration of deltachain and related classes
// no-check-code

#ifndef FBHGEXT_DELTACHAIN_H
#define FBHGEXT_DELTACHAIN_H

extern "C" {
#include "lib/cdatapack/cdatapack.h"
}

#include <memory>
#include <vector>

#include "edenscm/hgext/extlib/cstore/key.h"

/*
 * Wrapper around delta chain link, both C and Python
 */
class DeltaChainLink {
 private:
  const char *_filename, *_deltabasefilename;
  const uint8_t *_node, *_deltabasenode, *_delta;
  uint16_t _filenamesz, _deltabasefilenamesz;
  uint64_t _deltasz;

 public:
  explicit DeltaChainLink(delta_chain_link_t* link) {
    if (link) {
      _filename = link->filename;
      _deltabasefilename = link->filename;
      _node = link->node;
      _deltabasenode = link->deltabase_node;
      _delta = link->delta;
      _filenamesz = link->filename_sz;
      _deltabasefilenamesz = link->filename_sz;
      _deltasz = link->delta_sz;
    } else {
      _filename = NULL;
      _deltabasefilename = NULL;
      _node = NULL;
      _deltabasenode = NULL;
      _delta = NULL;
      _filenamesz = 0;
      _deltabasefilenamesz = 0;
      _deltasz = 0;
    }
  }

  DeltaChainLink(
      const char* filename,
      const char* deltabasefilename,
      const uint8_t* node,
      const uint8_t* deltabasenode,
      const uint8_t* delta,
      uint16_t filenamesz,
      uint16_t deltabasefilenamesz,
      uint64_t deltasz)
      : _filename(filename),
        _deltabasefilename(deltabasefilename),
        _node(node),
        _deltabasenode(deltabasenode),
        _delta(delta),
        _filenamesz(filenamesz),
        _deltabasefilenamesz(deltabasefilenamesz),
        _deltasz(deltasz) {}

  ~DeltaChainLink() = default;

  const char* filename() const {
    return _filename;
  }

  const char* deltabasefilename() const {
    return _deltabasefilename;
  }

  const uint8_t* node() const {
    return _node;
  }

  const uint8_t* deltabasenode() const {
    return _deltabasenode;
  }

  const uint8_t* delta() const {
    return _delta;
  }

  uint16_t filenamesz() const {
    return _filenamesz;
  }

  uint16_t deltabasefilenamesz() const {
    return _deltabasefilenamesz;
  }

  uint64_t deltasz() const {
    return _deltasz;
  }

  bool isdone() const {
    return (_filename == NULL);
  }
};

/*
 * Abstract delta chain class
 */
class DeltaChain {
 protected:
  DeltaChain() {}

 public:
  virtual ~DeltaChain() {}

  virtual const DeltaChainLink getlink(const size_t) const = 0;

  virtual size_t linkcount() const = 0;

  virtual get_delta_chain_code_t status() const = 0;
};

/*
 * Wrapper around C delta chain
 * CDeltaChain takes ownership of delta_chain_t
 */
class CDeltaChain : public DeltaChain {
 private:
  delta_chain_t _chain;

 public:
  // The constructor does a shallow copy of the delta chain and since the
  // ownership is taken by this class it is responsible for memory management
  explicit CDeltaChain(delta_chain_t chain) : _chain(chain) {}

  explicit CDeltaChain(get_delta_chain_code_t /*error*/)
      : _chain{GET_DELTA_CHAIN_NOT_FOUND, nullptr, 0} {}

  // Memory of _chain has to be deallocated because it is a C struct that
  // contains an array of delta_chain_link_t's
  ~CDeltaChain() {
    freedeltachain(_chain);
  }

  const DeltaChainLink getlink(const size_t idx) const override {
    return DeltaChainLink(&(_chain.delta_chain_links[idx]));
  }

  size_t linkcount() const override {
    return _chain.links_count;
  }

  get_delta_chain_code_t status() const override {
    return _chain.code;
  }
};

class DeltaChainIterator {
 private:
  size_t _index;

 protected:
  std::vector<std::shared_ptr<DeltaChain>> _chains;
  DeltaChainIterator() : _index(0) {}
  virtual std::shared_ptr<DeltaChain> getNextChain(const Key& /*key*/) {
    return std::make_shared<CDeltaChain>(GET_DELTA_CHAIN_NOT_FOUND);
  }

 public:
  explicit DeltaChainIterator(std::shared_ptr<DeltaChain> chain) : _index(0) {
    _chains.push_back(chain);
  }
  virtual ~DeltaChainIterator();

  DeltaChainLink next();
};

#endif // FBHGEXT_DELTACHAIN_H
