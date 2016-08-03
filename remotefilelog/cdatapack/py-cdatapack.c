// py-cdatapack.cpp - C implementation of a datapack
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include <Python.h>

#include "cdatapack.h"

#define DATAIDX_EXT  ".dataidx"
#define DATAPACK_EXT ".datapack"

// ====  py_cdatapack PyObject declaration ====

typedef struct {
  PyObject_HEAD;

  bool initialized;
  datapack_handle_t *handle;
} py_cdatapack;

/**
 * Initializes a cdatapack
 */
static int cdatapack_init(py_cdatapack *self, PyObject *args) {
  self->handle = NULL;

  char *node;
  ssize_t nodelen;

  if (!PyArg_ParseTuple(args, "s#", &node, &nodelen)) {
    return -1;
  }

  char idx_path[nodelen + sizeof(DATAIDX_EXT)];
  char data_path[nodelen + sizeof(DATAPACK_EXT)];

  self->handle = open_datapack(
      idx_path, strlen(idx_path),
      data_path, strlen(data_path));
  // TODO: error handling

  return 0;
}

/**
 * Deallocates a cdatapack
 */
static void cdatapack_dealloc(py_cdatapack *self) {
  if (self->handle != NULL) {
    close_datapack(self->handle);
  }
  PyObject_Del(self);
}

// ====  cdatapack ctype declaration ====

static PyMethodDef cdatapack_methods[] = {
    {NULL, NULL}
};

static PyTypeObject cdatapack_type = {
  PyObject_HEAD_INIT(NULL)
  0,                                    /* ob_size */
  "cdatapack.datapack",                 /* tp_name */
  sizeof(py_cdatapack),             /* tp_basicsize */
  0,                                    /* tp_itemsize */
  (destructor)cdatapack_dealloc,        /* tp_dealloc */
  0,                                    /* tp_print */
  0,                                    /* tp_getattr */
  0,                                    /* tp_setattr */
  0,                                    /* tp_compare */
  0,                                    /* tp_repr */
  0,                                    /* tp_as_number */
  0,                                    /* tp_as_sequence - length/contains */
  0,                                    /* tp_as_mapping - getitem/setitem*/
  0,                                    /* tp_hash */
  0,                                    /* tp_call */
  0,                                    /* tp_str */
  0,                                    /* tp_getattro */
  0,                                    /* tp_setattro */
  0,                                    /* tp_as_buffer */
  Py_TPFLAGS_DEFAULT,                   /* tp_flags */
  "TODO",                               /* tp_doc */
  0,                                    /* tp_traverse */
  0,                                    /* tp_clear */
  0,                                    /* tp_richcompare */
  0,                                    /* tp_weaklistoffset */
  0,                                    /* tp_iter */
  0,                                    /* tp_iternext */
  cdatapack_methods,                    /* tp_methods */
  0,                                    /* tp_members */
  0,                                    /* tp_getset */
  0,                                    /* tp_base */
  0,                                    /* tp_dict */
  0,                                    /* tp_descr_get */
  0,                                    /* tp_descr_set */
  0,                                    /* tp_dictoffset */
  (initproc)cdatapack_init,             /* tp_init */
  0,                                    /* tp_alloc */
};

static PyMethodDef mod_methods[] = {
  {NULL, NULL}
};

static char mod_description[] =
    "Module containing a native datapack implementation";

PyMODINIT_FUNC initcdatapack(void) {
  PyObject *mod;

  cdatapack_type.tp_new = PyType_GenericNew;
  if (PyType_Ready(&cdatapack_type) < 0) {
    return;
  }

  mod = Py_InitModule3("cdatapack", mod_methods, mod_description);

  Py_INCREF(&cdatapack_type);
  PyModule_AddObject(mod, "datapack", (PyObject *)&cdatapack_type);
}
