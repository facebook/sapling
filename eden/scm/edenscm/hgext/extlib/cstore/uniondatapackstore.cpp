/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// uniondatapackstore.cpp - implementation of a union datapack store
// no-check-code

#include "edenscm/hgext/extlib/cstore/uniondatapackstore.h"
#include "edenscm/hgext/extlib/cstore/util.h"

#include <algorithm>
#include <memory>

extern "C" {
#include "edenscm/mercurial/mpatch.h"
}

UnionDatapackStore::UnionDatapackStore() {}

UnionDatapackStore::UnionDatapackStore(std::vector<DataStore*>& stores)
    : _stores(stores) {}

UnionDatapackStore::~UnionDatapackStore() {
  // TODO: we should manage the substore lifetimes here, but because they are
  // also controlled by Python, we need to let python handle it and manage the
  // refcount in the py_uniondatapackstore type.
}

mpatch_flist* getNextLink(void* container, ssize_t index) {
  std::vector<DeltaChainLink>* links = (std::vector<DeltaChainLink>*)container;

  if (index < 0 || (size_t)index >= links->size()) {
    return NULL;
  }

  DeltaChainLink link = links->at(index);

  struct mpatch_flist* res;
  if ((mpatch_decode(
          (const char*)link.delta(), (ssize_t)link.deltasz(), &res)) < 0) {
    throw std::logic_error("invalid patch during patch application");
  }

  return res;
}

ConstantStringRef UnionDatapackStore::get(const Key& key) {
  UnionDeltaChainIterator chain = this->getDeltaChain(key);

  std::vector<DeltaChainLink> links;

  for (DeltaChainLink link = chain.next(); !link.isdone();
       link = chain.next()) {
    links.push_back(link);
  }

  DeltaChainLink fulltextLink = links.back();
  links.pop_back();

  // Short circuit and just return the full text if it's one long
  if (links.size() == 0) {
    return ConstantStringRef(
        (const char*)fulltextLink.delta(), (size_t)fulltextLink.deltasz());
  }

  std::reverse(links.begin(), links.end());

  mpatch_flist* patch = mpatch_fold(&links, getNextLink, 0, links.size());
  if (!patch) { /* error already set or memory error */
    throw std::logic_error("mpatch failed to fold patches");
  }

  ssize_t outlen = mpatch_calcsize((ssize_t)fulltextLink.deltasz(), patch);
  if (outlen < 0) {
    mpatch_lfree(patch);
    throw std::logic_error("mpatch failed to calculate size");
  }

  auto result = std::make_shared<std::string>(outlen, '\0');
  if (mpatch_apply(
          &(*result)[0],
          (const char*)fulltextLink.delta(),
          (ssize_t)fulltextLink.deltasz(),
          patch) < 0) {
    mpatch_lfree(patch);
    throw std::logic_error("mpatch failed to apply patches");
  }

  mpatch_lfree(patch);
  return ConstantStringRef(result);
}

std::shared_ptr<DeltaChain> UnionDeltaChainIterator::getNextChain(
    const Key& key) {
  for (std::vector<DataStore*>::iterator it = _store._stores.begin();
       it != _store._stores.end();
       it++) {
    DataStore* substore = *it;
    std::shared_ptr<DeltaChain> chain = substore->getDeltaChainRaw(key);

    if (chain->status() == GET_DELTA_CHAIN_OK) {
      return chain;
    }
  }

  throw MissingKeyError("unable to find delta chain");
}

UnionDeltaChainIterator UnionDatapackStore::getDeltaChain(const Key& key) {
  return UnionDeltaChainIterator(*this, key);
}

Key* UnionDatapackStoreKeyIterator::next() {
  Key* key;
  while ((key = _missing.next()) != NULL) {
    if (!_store.contains(*key)) {
      return key;
    }
  }

  return NULL;
}

bool UnionDatapackStore::contains(const Key& key) {
  for (std::vector<DataStore*>::iterator it = _stores.begin();
       it != _stores.end();
       it++) {
    DataStore* substore = *it;
    if (substore->contains(key)) {
      return true;
    }
  }
  return false;
}

UnionDatapackStoreKeyIterator UnionDatapackStore::getMissing(
    KeyIterator& missing) {
  return UnionDatapackStoreKeyIterator(*this, missing);
}

void UnionDatapackStore::markForRefresh() {
  for (std::vector<DataStore*>::iterator it = _stores.begin();
       it != _stores.end();
       it++) {
    DataStore* substore = *it;
    substore->markForRefresh();
  }
}

void UnionDatapackStore::addStore(DataStore* store) {
  _stores.push_back(store);
}

void UnionDatapackStore::removeStore(DataStore* store) {
  removeFromVector<DataStore*>(_stores, store);
}
