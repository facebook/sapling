/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// pythonutil.cpp - utilities to glue C++ code to python
// no-check-code

#include "pythonutil.h"

PythonObj::PythonObj() : obj(NULL) {}

PythonObj::PythonObj(PyObject* obj) {
  if (!obj) {
    if (!PyErr_Occurred()) {
      PyErr_SetString(
          PyExc_RuntimeError, "attempted to construct null PythonObj");
    }
    throw pyexception();
  }
  this->obj = obj;
}

PythonObj::PythonObj(const PythonObj& other) {
  this->obj = other.obj;
  Py_XINCREF(this->obj);
}

PythonObj::~PythonObj() {
  Py_XDECREF(this->obj);
}

PythonObj& PythonObj::operator=(const PythonObj& other) {
  Py_XDECREF(this->obj);
  this->obj = other.obj;
  Py_XINCREF(this->obj);
  return *this;
}

bool PythonObj::operator==(const PythonObj& other) const {
  return this->obj == other.obj;
}

PythonObj::operator PyObject*() const {
  return this->obj;
}

PythonObj::operator bool() const {
  return this->obj != NULL;
}

/**
 * Function used to obtain a return value that will persist beyond the life
 * of the PythonObj. This is useful for returning objects to Python C apis
 * and letting them manage the remaining lifetime of the object.
 */
PyObject* PythonObj::returnval() {
  Py_XINCREF(this->obj);
  return this->obj;
}

/**
 * Invokes getattr to retrieve the attribute from the python object.
 */
PythonObj PythonObj::getattr(const char* name) {
  return PyObject_GetAttrString(this->obj, name);
}

/**
 * Executes the current callable object if it's callable.
 */
PythonObj PythonObj::call(const PythonObj& args) {
  PyObject* result = PyEval_CallObject(this->obj, args);
  return PythonObj(result);
}

/**
 * Invokes the specified method on this instance.
 */
PythonObj PythonObj::callmethod(const char* name, const PythonObj& args) {
  PythonObj function = this->getattr(name);
  return PyObject_CallObject(function, args);
}

PythonStore::PythonStore(PythonObj store)
    : _get(store.getattr("get")), _storeObj(store) {}

PythonStore::PythonStore(const PythonStore& store)
    : _get(store._get), _storeObj(store._storeObj) {}

ConstantStringRef PythonStore::get(const Key& key) {
  PythonObj arglist = Py_BuildValue(
      "s#s#",
      key.name.c_str(),
      (Py_ssize_t)key.name.size(),
      key.node,
      (Py_ssize_t)BIN_NODE_SIZE);

  PyObject* result = PyEval_CallObject(_get, arglist);

  if (!result) {
    if (PyErr_Occurred()) {
      throw pyexception();
    }

    PyErr_Format(
        PyExc_RuntimeError,
        "unable to find tree '%.*s:...'",
        (int)key.name.size(),
        key.name.c_str());
    throw pyexception();
  }

  PythonObj resultobj(result);

  char* path;
  Py_ssize_t pathlen;
  if (PyString_AsStringAndSize((PyObject*)result, &path, &pathlen)) {
    throw pyexception();
  }

  return ConstantStringRef(path, pathlen);
}

bool PythonMatcher::matches(const std::string& path) {
  PythonObj matchArgs =
      Py_BuildValue("(s#)", path.c_str(), (Py_ssize_t)path.size());
  PythonObj matched = this->_matcherObj.call(matchArgs);
  return PyObject_IsTrue(matched) == 1;
}

bool PythonMatcher::matches(const char* path, const size_t pathlen) {
  PythonObj matchArgs = Py_BuildValue("(s#)", path, (Py_ssize_t)pathlen);
  PythonObj matched = this->_matcherObj.call(matchArgs);
  return PyObject_IsTrue(matched) == 1;
}

bool PythonMatcher::visitdir(const std::string& path) {
  Py_ssize_t size = path.size();
  if (size > 1 && path[size - 1] == '/') {
    size--;
  }

  PythonObj matchArgs = Py_BuildValue("(s#)", path.c_str(), (Py_ssize_t)size);
  PythonObj matched = this->_matcherObj.callmethod("visitdir", matchArgs);
  return PyObject_IsTrue(matched) == 1;
}

void PythonDiffResult::add(
    const std::string& path,
    const char* beforeNode,
    const char* beforeFlag,
    const char* afterNode,
    const char* afterFlag) {
  Py_ssize_t beforeLen = beforeNode != NULL ? BIN_NODE_SIZE : 0;
  Py_ssize_t afterLen = afterNode != NULL ? BIN_NODE_SIZE : 0;

  PythonObj entry = Py_BuildValue(
      "((s#s#)(s#s#))",
      beforeNode,
      beforeLen,
      (beforeFlag == NULL) ? MAGIC_EMPTY_STRING : beforeFlag,
      Py_ssize_t(beforeFlag ? 1 : 0),
      afterNode,
      afterLen,
      (afterFlag == NULL) ? MAGIC_EMPTY_STRING : afterFlag,
      Py_ssize_t(afterFlag ? 1 : 0));

  PythonObj pathObj = PyString_FromStringAndSize(path.c_str(), path.length());

  if (PyDict_SetItem(this->_diff, pathObj, entry)) {
    throw pyexception();
  }
}

void PythonDiffResult::addclean(const std::string& path) {
  PythonObj pathObj = PyString_FromStringAndSize(path.c_str(), path.length());
  Py_INCREF(Py_None);
  if (PyDict_SetItem(this->_diff, pathObj, Py_None)) {
    throw pyexception();
  }
}
