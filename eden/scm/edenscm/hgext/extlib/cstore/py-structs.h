/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// py-structs.h - c++ headers for store python objects
// no-check-code

#ifndef FBHGEXT_CSTORE_PY_STRUCTS_H
#define FBHGEXT_CSTORE_PY_STRUCTS_H

#include <memory>

#include "edenscm/hgext/extlib/cstore/datapackstore.h"
#include "edenscm/hgext/extlib/cstore/pythondatastore.h"
#include "edenscm/hgext/extlib/cstore/pythonutil.h"
#include "edenscm/hgext/extlib/cstore/uniondatapackstore.h"

// clang-format off
// clang thinks that PyObject_HEAD should be on the same line as the next line
// since there is no semicolong after it. There is no semicolon because
// PyObject_HEAD macro already contains one and MSVC does not support
// extra semicolons.
struct py_datapackstore {
  PyObject_HEAD

  DatapackStore datapackstore;
};
// clang-format on

// clang-format off
struct py_uniondatapackstore {
  PyObject_HEAD

  std::shared_ptr<UnionDatapackStore> uniondatapackstore;

  // Keep a reference to the python objects so we can decref them later.
  std::vector<PythonObj> cstores;
  std::vector<std::shared_ptr<PythonDataStore>> pystores;
};
// clang-format on

#endif // FBHGEXT_CSTORE_PY_STRUCTS_H
