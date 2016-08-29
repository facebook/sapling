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

// ====  Forward declarations ====

static PyTypeObject cdatapack_iterator_type;
static PyTypeObject cdatapack_deltas_iterator_type;

// ====  py_cdatapack PyObject declaration ====

typedef struct {
  PyObject_HEAD;

  bool initialized;
  datapack_handle_t *handle;
} py_cdatapack;

// ====  py_cdatapack_iterator PyObject declaration ====

typedef struct {
  PyObject_HEAD;

  py_cdatapack *datapack;
  const uint8_t *ptr;
  const uint8_t *end;
} py_cdatapack_iterator;

/**
 * Initializes a cdatapack
 */
static int cdatapack_init(py_cdatapack *self, PyObject *args) {
  self->handle = NULL;

  char *node;
  int nodelen;

  if (!PyArg_ParseTuple(args, "s#", &node, &nodelen)) {
    return -1;
  }

  char idx_path[nodelen + sizeof(DATAIDX_EXT)];
  char data_path[nodelen + sizeof(DATAPACK_EXT)];

  sprintf(idx_path, "%s%s", node, DATAIDX_EXT);
  sprintf(data_path, "%s%s", node, DATAPACK_EXT);

  self->handle = open_datapack(
      idx_path, strlen(idx_path),
      data_path, strlen(data_path));

  if (self->handle == NULL) {
    PyErr_Format(PyExc_ValueError,
        "Error setting up datapack");
    return -1;
  }
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

/**
 * Returns an iterator for a cdatapack.
 */
static py_cdatapack_iterator *cdatapack_getiter(py_cdatapack *self) {
  py_cdatapack_iterator *iterator;

  iterator = PyObject_New(py_cdatapack_iterator, &cdatapack_iterator_type);
  if (iterator == NULL) {
    return NULL;
  }

  iterator->datapack = self;
  Py_INCREF(iterator->datapack);
  /* TODO: should have a data_version type and use sizeof(..) */
  iterator->ptr = ((uint8_t *) self->handle->data_mmap) + 1;
  iterator->end = ((uint8_t *) self->handle->data_mmap) +
                  self->handle->data_file_sz;

  return iterator;
}

/**
 * Returns a delta iterator for a cdatapack.
 */
static py_cdatapack_iterator *cdatapack_getiterentries(py_cdatapack *self) {
  py_cdatapack_iterator *iterator;

  iterator = PyObject_New(
      py_cdatapack_iterator,
      &cdatapack_deltas_iterator_type);
  if (iterator == NULL) {
    return NULL;
  }

  iterator->datapack = self;
  Py_INCREF(iterator->datapack);
  /* TODO: should have a data_version type and use sizeof(..) */
  iterator->ptr = ((uint8_t *) self->handle->data_mmap) + 1;
  iterator->end = ((uint8_t *) self->handle->data_mmap) +
                  self->handle->data_file_sz;

  return iterator;
}

/**
 * Finds a node and returns a (node, deltabase index offset, data offset,
 * data size) tuple if found.
 */
static PyObject *cdatapack_find(
    py_cdatapack *self,
    PyObject *args) {
  const char *node;
  int node_sz;

  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }

  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  pack_index_entry_t pack_index_entry;

  if (find(self->handle, (const uint8_t *) node, &pack_index_entry) == false) {
    Py_RETURN_NONE;
  }

  PyObject *tuple = NULL;
  PyObject *retnode = NULL,
      *deltabaseindexoffset = NULL,
      *data_offset = NULL,
      *data_size = NULL;

  retnode = PyString_FromStringAndSize(
      (const char *) pack_index_entry.node, NODE_SZ);
  deltabaseindexoffset = PyInt_FromLong(
      pack_index_entry.deltabase_index_offset);
  data_offset = PyLong_FromLongLong(pack_index_entry.data_offset);
  data_size = PyLong_FromLongLong(pack_index_entry.data_sz);

  if (retnode == NULL || deltabaseindexoffset == NULL ||
      data_offset == NULL || data_size == NULL) {
    goto cleanup;
  }
  tuple = PyTuple_Pack(4, retnode, deltabaseindexoffset, data_offset, data_size);

  cleanup:

  Py_XDECREF(retnode);
  Py_XDECREF(deltabaseindexoffset);
  Py_XDECREF(data_offset);
  Py_XDECREF(data_size);

  return tuple;
}

/**
 * Finds a node and returns a list of (filename, node, filename, delta base
 * node, delta) tuples if found.
 */
static PyObject *cdatapack_getdeltachain(
    py_cdatapack *self,
    PyObject *args) {
  const char *node;
  int node_sz;

  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }

  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  delta_chain_t *chain = getdeltachain(self->handle, (const uint8_t *) node);
  if (chain == NULL) {
    Py_RETURN_NONE;
  }
  PyObject *result = PyList_New(chain->links_count);
  // TODO: error checking

  for (int ix = 0; ix < chain->links_count; ix ++) {
    PyObject *tuple = NULL;
    PyObject *name = NULL, *retnode = NULL, *deltabasenode = NULL, *delta =
        NULL;

    delta_chain_link_t *link = &chain->delta_chain_links[ix];

    name = PyString_FromStringAndSize(link->filename, link->filename_sz);
    retnode = PyString_FromStringAndSize((const char *) link->node, NODE_SZ);
    deltabasenode = PyString_FromStringAndSize(
        (const char *) link->deltabase_node, NODE_SZ);
    delta = PyString_FromStringAndSize(
        (const char *) link->delta, link->delta_sz);

    if (name != NULL &&
        retnode != NULL &&
        deltabasenode != NULL &&
        delta != NULL) {
      tuple = PyTuple_Pack(5, name, retnode, name, deltabasenode, delta);
    }

    Py_XDECREF(name);
    Py_XDECREF(retnode);
    Py_XDECREF(deltabasenode);
    Py_XDECREF(delta);

    if (tuple == NULL) {
      goto err_cleanup;
    }

    PyList_SetItem(result, ix, tuple);
  }

  goto cleanup;

err_cleanup:
  Py_XDECREF(result);
  result = NULL;

cleanup:
  freedeltachain(chain);
  return result;
}

// ====  cdatapack ctype declaration ====

static PyMethodDef cdatapack_methods[] = {
    {"iterentries", (PyCFunction)cdatapack_getiterentries,
        METH_NOARGS,
        "Iterate over (path, nodeid, deltabasenode, delta) tuples in this "
            "datapack."},
    {"_find", (PyCFunction)cdatapack_find,
        METH_VARARGS,
        "Finds a node and returns a (node, deltabase index offset, "
            "data offset, data size) tuple if found."},
    {"getdeltachain", (PyCFunction)cdatapack_getdeltachain,
        METH_VARARGS,
        "Finds a node and returns a list of (filename, node, filename, delta "
            "base node, delta) tuples if found."},
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
  (getiterfunc) cdatapack_getiter,      /* tp_iter */
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

// ====  cdatapack_iterator class methods ====

/**
 * Deallocates a cdatapack iterator
 */
static void cdatapack_iterator_dealloc(py_cdatapack_iterator *self) {
  Py_XDECREF(self->datapack);
  PyObject_Del(self);
}

/**
 * Yields the next item from the iterator.
 */
static PyObject *cdatapack_iterator_iternext(py_cdatapack_iterator *iterator) {
  delta_chain_link_t link;

  if (iterator->ptr >= iterator->end) {
    return NULL;
  }

  iterator->ptr = getdeltachainlink(iterator->ptr, &link);

  PyObject *tuple = NULL, *fn = NULL, *node = NULL;

  fn = PyString_FromStringAndSize(link.filename, link.filename_sz);
  node = PyString_FromStringAndSize((const char *) link.node, NODE_SZ);
  if (fn == NULL || node == NULL) {
    goto cleanup;
  }
  tuple = PyTuple_Pack(2, fn, node);

cleanup:

  Py_XDECREF(fn);
  Py_XDECREF(node);

  return tuple;
}

// ====  cdatapack_iterator ctype declaration ====

static PyTypeObject cdatapack_iterator_type = {
  PyObject_HEAD_INIT(NULL)
  0,                                    /* ob_size */
  "cdatapack.datapack.iterator",        /* tp_name */
  sizeof(py_cdatapack_iterator),        /* tp_basicsize */
  0,                                    /* tp_itemsize */
  (destructor)cdatapack_iterator_dealloc, /* tp_dealloc */
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
  "Iterator for entries-tuples in a datapack.", /* tp_doc */
  0,                                    /* tp_traverse */
  0,                                    /* tp_clear */
  0,                                    /* tp_richcompare */
  0,                                    /* tp_weaklistoffset */
  PyObject_SelfIter,                    /* tp_iter: __iter__() method */
  (iternextfunc) cdatapack_iterator_iternext, /* tp_iternext: next() method */
};

// ====  cdatapack_deltas_iterator class methods ====

/**
 * Deallocates a cdatapack deltas iterator
 */
static void cdatapack_deltas_iterator_dealloc(py_cdatapack_iterator *self) {
  Py_XDECREF(self->datapack);
  PyObject_Del(self);
}

/**
 * Yields the next item from the iterator.
 */
static PyObject *cdatapack_deltas_iterator_iternext(
    py_cdatapack_iterator *iterator) {
  delta_chain_link_t link;

  if (iterator->ptr >= iterator->end) {
    return NULL;
  }

  iterator->ptr = getdeltachainlink(iterator->ptr, &link);

  PyObject *tuple = NULL;
  PyObject *fn = NULL, *node = NULL, *deltabasenode = NULL, *deltalen = NULL;

  fn = PyString_FromStringAndSize(link.filename, link.filename_sz);
  node = PyString_FromStringAndSize((const char *) link.node, NODE_SZ);
  deltabasenode = PyString_FromStringAndSize((const char *) link.deltabase_node,
      NODE_SZ);
  deltalen = PyLong_FromLongLong(link.delta_sz);
  if (fn == NULL || node == NULL || deltabasenode == NULL || deltalen == NULL) {
    goto cleanup;
  }
  tuple = PyTuple_Pack(4, fn, node, deltabasenode, deltalen);

cleanup:

  Py_XDECREF(fn);
  Py_XDECREF(node);
  Py_XDECREF(deltabasenode);
  Py_XDECREF(deltalen);

  return tuple;
}

// ====  cdatapack_deltas_iterator ctype declaration ====

static PyTypeObject cdatapack_deltas_iterator_type = {
    PyObject_HEAD_INIT(NULL)
    0,                                    /* ob_size */
    "cdatapack.datapack.iterentries",     /* tp_name */
    sizeof(py_cdatapack_iterator),        /* tp_basicsize */
    0,                                    /* tp_itemsize */
    (destructor)cdatapack_deltas_iterator_dealloc, /* tp_dealloc */
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
    "Iterator for delta chains in a datapack.", /* tp_doc */
    0,                                    /* tp_traverse */
    0,                                    /* tp_clear */
    0,                                    /* tp_richcompare */
    0,                                    /* tp_weaklistoffset */
    PyObject_SelfIter,                    /* tp_iter: __iter__() method */
    (iternextfunc) cdatapack_deltas_iterator_iternext, /* tp_iternext: next()
                                                        * method */
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
