/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// pythonutil.h - utilities to glue C++ code to python
// no-check-code

#ifndef FBHGEXT_CSTORE_PYTHONUTIL_H
#define FBHGEXT_CSTORE_PYTHONUTIL_H

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN

#include <Python.h>
#include <exception>

// Py_BuildValue treats NULL as NONE, so we have to have a non-null pointer.
#define MAGIC_EMPTY_STRING ""

#include "edenscm/hgext/extlib/cstore/key.h"
#include "edenscm/hgext/extlib/cstore/match.h"
#include "edenscm/hgext/extlib/cstore/store.h"
#include "edenscm/hgext/extlib/ctreemanifest/treemanifest.h"

/**
 * C++ exception that represents an issue at the python C api level.
 * When this is thrown, it's assumed that the python error message has been set
 * and that the catcher of the exception should just return an error code value
 * to the python API.
 */
class pyexception : public std::exception {
 public:
  pyexception() {}
};

/**
 * Wrapper class for PyObject pointers.
 * It is responsible for managing the Py_INCREF and Py_DECREF calls.
 */
class PythonObj {
 private:
  PyObject* obj;

 public:
  PythonObj();

  PythonObj(PyObject* obj);

  PythonObj(const PythonObj& other);

  ~PythonObj();

  PythonObj& operator=(const PythonObj& other);

  bool operator==(const PythonObj& other) const;

  operator PyObject*() const;

  operator bool() const;

  /**
   * Function used to obtain a return value that will persist beyond the life
   * of the PythonObj. This is useful for returning objects to Python C apis
   * and letting them manage the remaining lifetime of the object.
   */
  PyObject* returnval();

  /**
   * Invokes getattr to retrieve the attribute from the python object.
   */
  PythonObj getattr(const char* name);

  /**
   * Executes the current callable object if it's callable.
   */
  PythonObj call(const PythonObj& args);

  /**
   * Invokes the specified method on this instance.
   */
  PythonObj callmethod(const char* name, const PythonObj& args);
};

class PythonStore : public Store {
 private:
  PythonObj _get;
  PythonObj _storeObj;

 public:
  PythonStore(PythonObj store);

  PythonStore(const PythonStore& store);

  virtual ~PythonStore() {}

  ConstantStringRef get(const Key& key);
};

class PythonMatcher : public Matcher {
 private:
  PythonObj _matcherObj;

 public:
  PythonMatcher(PythonObj matcher) : _matcherObj(matcher) {}

  virtual ~PythonMatcher() {}

  bool matches(const std::string& path);
  bool matches(const char* path, const size_t pathlen);
  bool visitdir(const std::string& path);
};

class PythonDiffResult : public DiffResult {
 private:
  PythonObj _diff;

 public:
  PythonDiffResult(PythonObj diff) : _diff(diff) {}
  virtual void add(
      const std::string& path,
      const char* beforeNode,
      const char* beforeFlag,
      const char* afterNode,
      const char* afterFlag);
  virtual void addclean(const std::string& path);
  PythonObj getDiff() {
    return this->_diff;
  }
};
#endif // FBHGEXT_CSTORE_PYTHONUTIL_H
