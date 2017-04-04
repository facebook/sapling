// uniondatapackstore.cpp - implementation of a union datapack store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include <algorithm>
#include <memory>

#include "uniondatapackstore.h"

extern "C" {
#include "mpatch.h"
}

UnionDatapackStore::UnionDatapackStore(std::vector<DatapackStore*> stores) :
  _stores(stores) {
}

UnionDatapackStore::~UnionDatapackStore() {
  // TODO: we should manage the substore lifetimes here, but because they are
  // also controlled by Python, we need to let python handle it and manage the
  // refcount in the py_uniondatapackstore type.
}

mpatch_flist* getNextLink(void* container, ssize_t index) {
  std::vector<delta_chain_link_t*> *links = (std::vector<delta_chain_link_t*>*)container;

  if (index < 0 || (size_t)index >= links->size()) {
    return NULL;
  }

  delta_chain_link_t *link = links->at(index);

  struct mpatch_flist *res;
  if ((mpatch_decode((const char*)link->delta,
		     (ssize_t)link->delta_sz, &res)) < 0) {
    throw std::logic_error("invalid patch during patch application");
  }

  return res;
}

ConstantStringRef UnionDatapackStore::get(const Key &key) {
  UnionDeltaChainIterator chain = this->getDeltaChain(key);

  std::vector<delta_chain_link_t*> links;

  delta_chain_link_t *link;
  while ((link = chain.next()) != NULL) {
    links.push_back(link);
  }

  delta_chain_link_t *fulltextLink = links.back();
  links.pop_back();

  // Short circuit and just return the full text if it's one long
  if (links.size() == 0) {
    char * finalText = new char[fulltextLink->delta_sz];
    memcpy(finalText, fulltextLink->delta, (size_t)fulltextLink->delta_sz);
    return ConstantStringRef(finalText, (size_t)fulltextLink->delta_sz);
  }

  std::reverse(links.begin(), links.end());

  mpatch_flist *patch = mpatch_fold(&links, getNextLink, 0, links.size());
  if (!patch) { /* error already set or memory error */
    throw std::logic_error("mpatch failed to fold patches");
  }

  ssize_t outlen = mpatch_calcsize((ssize_t)fulltextLink->delta_sz, patch);
  if (outlen < 0) {
    mpatch_lfree(patch);
    throw std::logic_error("mpatch failed to calculate size");
  }

  char *result= new char[outlen];
  if (mpatch_apply(result, (const char*)fulltextLink->delta,
		   (ssize_t)fulltextLink->delta_sz, patch) < 0) {
    delete[] result;
    mpatch_lfree(patch);
    throw std::logic_error("mpatch failed to apply patches");
  }

  mpatch_lfree(patch);
  return ConstantStringRef(result, outlen);
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

void UnionDatapackStore::markForRefresh() {
  for(std::vector<DatapackStore*>::iterator it = _stores.begin();
      it != _stores.end();
      it++) {
    DatapackStore *substore = *it;
    substore->markForRefresh();
  }
}
