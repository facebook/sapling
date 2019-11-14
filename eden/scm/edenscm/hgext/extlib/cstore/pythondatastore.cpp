/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// pythondatastore.cpp - implementation of a python data store
// no-check-code

#include "edenscm/hgext/extlib/cstore/pythondatastore.h"
#include "edenscm/hgext/extlib/cstore/pythonkeyiterator.h"

PythonDataStore::PythonDataStore(PythonObj store) : _store(store) {}

DeltaChainIterator PythonDataStore::getDeltaChain(const Key& key) {
  std::shared_ptr<DeltaChain> chain = getDeltaChainRaw(key);
  return DeltaChainIterator(chain);
}

std::shared_ptr<DeltaChain> PythonDataStore::getDeltaChainRaw(const Key& key) {
  // Extract the delta chain from the list of tuples
  // and build a DeltaChain object from them
  std::shared_ptr<std::vector<DeltaChainLink>> links =
      std::make_shared<std::vector<DeltaChainLink>>();

  std::shared_ptr<std::vector<PythonObj>> tuples =
      std::make_shared<std::vector<PythonObj>>();

  // Build (name, node) tuple and call getdeltachain
  // method of the underlying store
  PythonObj pyKey = Py_BuildValue(
      "(s#s#)", (key.name).c_str(), (key.name).size(), key.node, 20);
  PythonObj list;
  try {
    list = _store.callmethod("getdeltachain", pyKey);
  } catch (const pyexception& ex) {
    if (PyErr_ExceptionMatches(PyExc_KeyError)) {
      // Clear the exception, otherwise next method call will exit immediately
      PyErr_Clear();
      // Return empty Delta Chain which status is GET_DELTA_CHAIN_NOT_FOUND
      return std::make_shared<PyDeltaChain>(links, tuples);
    } else {
      // If this is not a KeyError exception then rethrow it
      throw;
    }
  }

  PythonObj iter = PyObject_GetIter(list);
  PyObject* item;
  while ((item = PyIter_Next(iter)) != NULL) {
    PythonObj tuple(item);

    const char *filename, *deltabasefilename;
    const uint8_t *node, *deltabasenode, *delta;
    uint16_t filenamesz, deltabasefilenamesz;
    uint64_t deltasz, nodesz, deltabasenodesz;

    if (!PyArg_ParseTuple(
            tuple,
            "s#z#s#z#z#",
            &filename,
            &filenamesz,
            &node,
            &nodesz,
            &deltabasefilename,
            &deltabasefilenamesz,
            &deltabasenode,
            &deltabasenodesz,
            &delta,
            &deltasz)) {
      throw pyexception();
    }

    links->push_back(DeltaChainLink(
        filename,
        deltabasefilename,
        node,
        deltabasenode,
        delta,
        filenamesz,
        deltabasefilenamesz,
        deltasz));

    tuples->push_back(tuple);
  }

  return std::make_shared<PyDeltaChain>(links, tuples);
}

std::shared_ptr<KeyIterator> PythonDataStore::getMissing(KeyIterator& missing) {
  PythonObj list = PyList_New(0);

  Key* key;
  while ((key = missing.next()) != NULL) {
    PythonObj pyKey = Py_BuildValue(
        "(s#s#)", key->name.c_str(), key->name.size(), key->node, 20);
    if (PyList_Append(list, (PyObject*)pyKey)) {
      throw pyexception();
    }
  }

  PythonObj arg = Py_BuildValue("(O)", (PyObject*)list);
  PythonObj keys = _store.callmethod("getmissing", arg);

  PythonObj iter = PyObject_GetIter((PyObject*)keys);
  return std::make_shared<PythonKeyIterator>(iter);
}

void PythonDataStore::markForRefresh() {
  PythonObj args = Py_BuildValue("");
  _store.callmethod("markforrefresh", args);
}

class Single : public KeyIterator {
 public:
  Key* _k;
  Single(Key* k) : _k(k) {}
  Key* next() override {
    Key* tmp = _k;
    _k = NULL;
    return tmp;
  }
};

bool PythonDataStore::contains(const Key& key) {
  Single iter((Key*)&key);
  std::shared_ptr<KeyIterator> it = getMissing(iter);
  return (!it->next());
}

PythonObj PythonDataStore::getStore() {
  return this->_store;
}
