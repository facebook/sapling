// py-structs.h - c++ headers for store python objects
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef CSTORE_PY_STRUCTS
#define CSTORE_PY_STRUCTS

#include <memory>

#include "datapackstore.h"
#include "uniondatapackstore.h"

struct py_datapackstore {
  PyObject_HEAD;

  DatapackStore datapackstore;
};

struct py_uniondatapackstore {
  PyObject_HEAD;

  std::shared_ptr<UnionDatapackStore> uniondatapackstore;

  // Keep a reference to the python objects so we can decref them later.
  std::vector<PythonObj> substores;
};

#endif //CSTORE_PY_STRUCTS
