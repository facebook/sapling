/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// pythonkeyiterator.h - c++ implementation of python key iterator
// no-check-code

#ifndef FBHGEXT_PYTHONKEYITERATOR_H
#define FBHGEXT_PYTHONKEYITERATOR_H

#include "edenscm/hgext/extlib/cstore/pythonutil.h"

class PythonKeyIterator : public KeyIterator {
 private:
  PythonObj _input;
  Key _current;

 public:
  PythonKeyIterator(PythonObj input) : _input(input) {}

  Key* next() {
    PyObject* item;
    while ((item = PyIter_Next((PyObject*)_input)) != NULL) {
      PythonObj itemObj(item);

      char* name;
      Py_ssize_t namelen;
      char* node;
      Py_ssize_t nodelen;
      if (!PyArg_ParseTuple(item, "s#s#", &name, &namelen, &node, &nodelen)) {
        throw pyexception();
      }

      _current = Key(name, namelen, node, nodelen);
      return &_current;
    }

    return NULL;
  }
};

#endif // FBHGEXT_PYTHONKEYITERATOR_H
