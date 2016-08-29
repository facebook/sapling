// pythonutil.h - utilities to glue C++ code to python
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef REMOTEFILELOG_PYTHONOBJ_H
#define REMOTEFILELOG_PYTHONOBJ_H

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN

#include <Python.h>
#include <exception>

/**
 * C++ exception that represents an issue at the python C api level.
 * When this is thrown, it's assumed that the python error message has been set
 * and that the catcher of the exception should just return an error code value
 * to the python API.
 */
class pyexception : public std::exception {
  public:
    pyexception() {
    }
};

/**
 * Wrapper class for PyObject pointers.
 * It is responsible for managing the Py_INCREF and Py_DECREF calls.
 */
class PythonObj {
  private:
    PyObject *obj;

  public:
    PythonObj();

    PythonObj(PyObject *obj);

    PythonObj(const PythonObj& other);

    ~PythonObj();

    PythonObj& operator=(const PythonObj &other);

    operator PyObject* () const;

    /**
     * Function used to obtain a return value that will persist beyond the life
     * of the PythonObj. This is useful for returning objects to Python C apis
     * and letting them manage the remaining lifetime of the object.
     */
    PyObject *returnval();

    /**
     * Invokes getattr to retrieve the attribute from the python object.
     */
    PythonObj getattr(const char *name);
};

#endif //REMOTEFILELOG_PYTHONOBJ_H
