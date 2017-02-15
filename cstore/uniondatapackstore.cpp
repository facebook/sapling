// uniondatapackstore.cpp - implementation of a union datapack store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include <algorithm>

#include "uniondatapackstore.h"

UnionDatapackStore::UnionDatapackStore(std::vector<DatapackStore*> stores) :
  _stores(stores) {
}

UnionDatapackStore::~UnionDatapackStore() {
  // TODO: we should manage the substore lifetimes here, but because they are
  // also controlled by Python, we need to let python handle it and manage the
  // refcount in the py_uniondatapackstore type.
}

delta_chain_t UnionDeltaChainIterator::getNextChain(const Key &key) {
  for(std::vector<DatapackStore*>::iterator it = _store._stores.begin();
      it != _store._stores.end();
      it++) {
    DatapackStore *substore = *it;
    delta_chain_t chain = substore->getDeltaChainRaw(key);
    if (chain.code == GET_DELTA_CHAIN_OK) {
      return chain;
    }
    freedeltachain(chain);
  }

  throw MissingKeyError("unable to find delta chain");
}

UnionDeltaChainIterator UnionDatapackStore::getDeltaChain(const Key &key) {
  return UnionDeltaChainIterator(*this, key);
}

Key *UnionDatapackStoreKeyIterator::next() {
  Key *key;
  while ((key = _missing.next()) != NULL) {
    if (!_store.contains(*key)) {
      return key;
    }
  }

  return NULL;
}

bool UnionDatapackStore::contains(const Key &key) {
  for(std::vector<DatapackStore*>::iterator it = _stores.begin();
      it != _stores.end();
      it++) {
    DatapackStore *substore = *it;
    if (substore->contains(key)) {
      return true;
    }
  }
  return false;
}

UnionDatapackStoreKeyIterator UnionDatapackStore::getMissing(KeyIterator &missing) {
  return UnionDatapackStoreKeyIterator(*this, missing);
}
