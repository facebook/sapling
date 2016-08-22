// manifest_fetcher.cpp - c++ implementation of a fetcher for manifests
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "manifest_fetcher.h"

ManifestFetcher::ManifestFetcher(PythonObj &store) :
    _get(store.getattr("get")) {
}

/**
 * Fetches the Manifest from the store for the provided manifest key.
 * Returns the manifest if found, or throws an exception if not found.
 */
Manifest *ManifestFetcher::get(const manifestkey &key) const {
  PythonObj arglist = Py_BuildValue("s#s#",
      key.path->c_str(), (Py_ssize_t)key.path->size(),
      key.node->c_str(), (Py_ssize_t)key.node->size());

  PyObject *result = PyEval_CallObject(this->_get, arglist);

  if (!result) {
    if (PyErr_Occurred()) {
      throw pyexception();
    }

    PyErr_Format(PyExc_RuntimeError, "unable to find tree '%s:...'", key.path->c_str());
    throw pyexception();
  }

  PythonObj resultobj(result);
  return new Manifest(resultobj);
}
