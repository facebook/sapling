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

class ConstantString {
  friend class ConstantStringRef;
  private:
    char *_content;
    size_t _size;
    size_t _refCount;

    ConstantString(char *content, size_t size) :
      _content(content),
      _size(size),
      _refCount(1) {}
  public:
    ~ConstantString() {
      delete _content;
    }

    char *content() {
      return _content;
    }

    size_t size() {
      return _size;
    }

    void incref() {
      _refCount++;
    }

    size_t decref() {
      if (_refCount > 0) {
        _refCount--;
      }
      return _refCount;
    }
};

class ConstantStringRef {
  private:
    ConstantString *_str;
  public:
    ConstantStringRef(char *str, size_t size) :
      _str(new ConstantString(str, size)) {
    }

    ConstantStringRef(const ConstantStringRef &other) {
      other._str->incref();
      _str = other._str;
    }

    ~ConstantStringRef() {
      if (_str->decref() == 0) {
        delete _str;
      }
    }

    ConstantStringRef& operator=(const ConstantStringRef &other) {
      if (_str->decref() == 0) {
        delete _str;
      }
      _str = other._str;
      _str->incref();
      return *this;
    }

    char *content() {
      return _str->content();
    }

    size_t size() {
      return _str->size();
    }
};

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

class UnionDeltaChainIterator: public DeltaChainIterator {
  private:
    UnionDatapackStore &_store;
  protected:
    delta_chain_t getNextChain(const Key &key);
  public:
    UnionDeltaChainIterator(UnionDatapackStore &store, const Key &key) :
      DeltaChainIterator(),
      _store(store) {
      _chains.push_back(this->getNextChain(key));
    }
};

class UnionDatapackStore {
  public:
    std::vector<DatapackStore*> _stores;

    UnionDatapackStore(std::vector<DatapackStore*> stores);

    ~UnionDatapackStore();

    ConstantStringRef get(const Key &key);

    UnionDeltaChainIterator getDeltaChain(const Key &key);

    bool contains(const Key &key);

    UnionDatapackStoreKeyIterator getMissing(KeyIterator &missing);
};

#endif //UNIONDATAPACKSTORE_H
