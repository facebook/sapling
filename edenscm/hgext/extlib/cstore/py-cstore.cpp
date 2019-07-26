// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// py-cstore.cpp - C++ implementation of a store
// no-check-code

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN

#include <Python.h>

#include "edenscm/hgext/extlib/cstore/py-datapackstore.h"
#include "edenscm/hgext/extlib/cstore/py-treemanifest.h"

static PyMethodDef mod_methods[] = {{NULL, NULL}};

static char mod_description[] =
    "Module containing a native store implementation";

PyMODINIT_FUNC initcstore(void) {
  PyObject* mod;

  mod = Py_InitModule3("cstore", mod_methods, mod_description);

  // Init treemanifest
  treemanifestType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&treemanifestType) < 0) {
    return;
  }
  Py_INCREF(&treemanifestType);
  PyModule_AddObject(mod, "treemanifest", (PyObject*)&treemanifestType);

  // Init datapackstore
  datapackstoreType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&datapackstoreType) < 0) {
    return;
  }
  Py_INCREF(&datapackstoreType);
  PyModule_AddObject(mod, "datapackstore", (PyObject*)&datapackstoreType);

  // Init datapackstore
  uniondatapackstoreType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&uniondatapackstoreType) < 0) {
    return;
  }
  Py_INCREF(&uniondatapackstoreType);
  PyModule_AddObject(
      mod, "uniondatapackstore", (PyObject*)&uniondatapackstoreType);
}
