/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// datastore.h - c++ declarations for a data store
// no-check-code

#ifndef FBHGEXT_DATASTORE_H
#define FBHGEXT_DATASTORE_H

#include <memory>

#include "edenscm/hgext/extlib/cstore/deltachain.h"
#include "edenscm/hgext/extlib/cstore/key.h"

class DataStore {
 protected:
  DataStore() {}

 public:
  virtual ~DataStore() {}

  virtual DeltaChainIterator getDeltaChain(const Key& key) = 0;

  virtual std::shared_ptr<DeltaChain> getDeltaChainRaw(const Key& key) = 0;

  virtual std::shared_ptr<KeyIterator> getMissing(KeyIterator& missing) = 0;

  virtual bool contains(const Key& key) = 0;

  virtual void markForRefresh() = 0;

  virtual void refresh() {}
};

#endif // FBHGEXT_DATASTORE_H
