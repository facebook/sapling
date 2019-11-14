/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// deltachain.cpp - c++ implementation of deltachain and related classes
// no-check-code

#include "edenscm/hgext/extlib/cstore/deltachain.h"

DeltaChainIterator::~DeltaChainIterator() {}

DeltaChainLink DeltaChainIterator::next() {
  std::shared_ptr<DeltaChain> chain = _chains.back();

  if (_index >= chain->linkcount()) {
    // If we're not at the end, and we have a callback to fetch more, do the
    // fetch.
    bool refreshed = false;
    if (chain->linkcount() > 0) {
      DeltaChainLink result = chain->getlink(_index - 1);

      const uint8_t* deltabasenode = result.deltabasenode();
      if (memcmp(deltabasenode, NULLID, BIN_NODE_SIZE) != 0) {
        Key key(
            result.filename(),
            result.filenamesz(),
            (const char*)deltabasenode,
            BIN_NODE_SIZE);

        std::shared_ptr<DeltaChain> newChain = this->getNextChain(key);
        if (newChain->status() == GET_DELTA_CHAIN_OK) {
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
