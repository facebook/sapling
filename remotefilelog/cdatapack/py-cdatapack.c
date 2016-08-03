// py-cdatapack.cpp - C implementation of a datapack
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include <Python.h>

static PyMethodDef mod_methods[] = {
  {NULL, NULL}
};

static char mod_description[] =
    "Module containing a native datapack implementation";

PyMODINIT_FUNC initcdatapack(void) {
  PyObject *mod;

  mod = Py_InitModule3("cdatapack", mod_methods, mod_description);
}
