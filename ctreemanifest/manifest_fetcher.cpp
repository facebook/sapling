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
ManifestPtr ManifestFetcher::get(
    const char *path, size_t pathlen,
    std::string &node) const {
  PythonObj arglist = Py_BuildValue("s#s#",
      path, (Py_ssize_t) pathlen,
      node.c_str(), (Py_ssize_t)node.size());

  PyObject *result = PyEval_CallObject(this->_get, arglist);

  if (!result) {
    if (PyErr_Occurred()) {
      throw pyexception();
    }

    PyErr_Format(PyExc_RuntimeError,
        "unable to find tree '%.*s:...'", (int) pathlen, path);
    throw pyexception();
  }

  PythonObj resultobj(result);
  return ManifestPtr(new Manifest(resultobj));
}
