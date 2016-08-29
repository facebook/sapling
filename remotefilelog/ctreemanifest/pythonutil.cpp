// pythonutil.cpp - utilities to glue C++ code to python
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "pythonutil.h"

PythonObj::PythonObj() :
    obj(NULL) {
}

PythonObj::PythonObj(PyObject *obj) {
  if (!obj) {
    if (!PyErr_Occurred()) {
      PyErr_SetString(PyExc_RuntimeError,
          "attempted to construct null PythonObj");
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

PythonObj& PythonObj::operator=(const PythonObj &other) {
  Py_XDECREF(this->obj);
  this->obj = other.obj;
  Py_XINCREF(this->obj);
  return *this;
}

PythonObj::operator PyObject* () const {
  return this->obj;
}

/**
 * Function used to obtain a return value that will persist beyond the life
 * of the PythonObj. This is useful for returning objects to Python C apis
 * and letting them manage the remaining lifetime of the object.
 */
PyObject *PythonObj::returnval() {
  Py_XINCREF(this->obj);
  return this->obj;
}

/**
 * Invokes getattr to retrieve the attribute from the python object.
 */
PythonObj PythonObj::getattr(const char *name) {
  return PyObject_GetAttrString(this->obj, name);
}

/**
 * Executes the current callable object if it's callable.
 */
PythonObj PythonObj::call(const PythonObj &args) {
  PyObject *result = PyEval_CallObject(this->obj, args);
  return PythonObj(result);
}

/**
 * Invokes the specified method on this instance.
 */
PythonObj PythonObj::callmethod(const char *name, const PythonObj &args) {
  PythonObj function = this->getattr(name);
  return PyObject_CallObject(function, args);
}
