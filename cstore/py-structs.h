// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// py-structs.h - c++ headers for store python objects
// no-check-code

#ifndef FBHGEXT_CSTORE_PY_STRUCTS_H
#define FBHGEXT_CSTORE_PY_STRUCTS_H

#include <memory>

#include "cstore/datapackstore.h"
#include "cstore/pythondatastore.h"
#include "cstore/pythonutil.h"
#include "cstore/uniondatapackstore.h"

struct py_datapackstore {
  PyObject_HEAD;

  DatapackStore datapackstore;
};

struct py_uniondatapackstore {
  PyObject_HEAD;

  std::shared_ptr<UnionDatapackStore> uniondatapackstore;

  // Keep a reference to the python objects so we can decref them later.
  std::vector<PythonObj> cstores;
  std::vector< std::shared_ptr<PythonDataStore> > pystores;
};

#endif // FBHGEXT_CSTORE_PY_STRUCTS_H
