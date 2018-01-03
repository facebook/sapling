// pythondatastore.h - c++ declarations for a python data store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.

#ifndef FBHGEXT_PYTHONDATASTORE_H
#define FBHGEXT_PYTHONDATASTORE_H

#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <memory>

#include "cstore/datastore.h"
#include "cstore/key.h"
#include "cstore/pythonutil.h"

/*
 * Wrapper around python delta chain
 */
class PyDeltaChain : public DeltaChain {
  private:
    std::shared_ptr< std::vector<DeltaChainLink> > _chain;
    std::shared_ptr< std::vector<PythonObj> > _pythonChainLinks;

  public:
    PyDeltaChain(std::shared_ptr< std::vector<DeltaChainLink> > chain,
                 std::shared_ptr< std::vector<PythonObj> > pythonChainLinks) :
      _chain(chain),
      _pythonChainLinks(pythonChainLinks) {}

    // Default destructor is used, because the destructor of _chain
    // and _tuples objects will free the allocated memory automatically.
    ~PyDeltaChain() {}

    const DeltaChainLink getlink(const size_t idx) {
      return _chain->at(idx);
    }

    size_t linkcount() {
      return _chain->size();
    }

    get_delta_chain_code_t status() {
      if (_chain->size()) {
        return GET_DELTA_CHAIN_OK;
      } else {
        return GET_DELTA_CHAIN_NOT_FOUND;
      }
    }

};

class PythonDataStore : public DataStore {
  private:
    PythonObj _store; // pointer to python object

  public:
    PythonDataStore(PythonObj store);

    ~PythonDataStore() = default;

    DeltaChainIterator getDeltaChain(const Key &key);

    std::shared_ptr<KeyIterator> getMissing(KeyIterator &missing);

    std::shared_ptr<DeltaChain> getDeltaChainRaw(const Key &key);

    bool contains(const Key &key);

    void markForRefresh();
};

#endif //FBHGEXT_PYTHONDATASTORE_H
