// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// py-cdatapack.h - python extension for cdatapack
// no-check-code

#ifndef FBHGEXT_CSTORE_PY_CDATAPACK_H
#define FBHGEXT_CSTORE_PY_CDATAPACK_H

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN

#include <Python.h>
#include <string>

extern "C" {
#include "lib/cdatapack/cdatapack.h"
#include "lib/clib/portability/inet.h"
}

// ====  py_cdatapack PyObject declaration ====

// clang-format off
// clang thinks that PyObject_HEAD should be on the same line as the next line
// since there is no semicolong after it. There is no semicolon because
// PyObject_HEAD macro already contains one and MSVC does not support
// extra semicolons.
struct py_cdatapack {
  PyObject_HEAD

  bool initialized;
  datapack_handle_t *handle;
};
// clang-format on

// ====  py_cdatapack_iterator PyObject declaration ====

// clang-format off
typedef struct {
  PyObject_HEAD

  py_cdatapack *datapack;
  const uint8_t *ptr;
  const uint8_t *end;
} py_cdatapack_iterator;
// clang-format on

// ====  cdatapack_deltas_iterator class methods ====

/**
 * Deallocates a cdatapack deltas iterator
 */
static void cdatapack_deltas_iterator_dealloc(py_cdatapack_iterator *self)
{
  Py_XDECREF(self->datapack);
  PyObject_Del(self);
}

/**
 * Yields the next item from the iterator.
 */
static PyObject *
cdatapack_deltas_iterator_iternext(py_cdatapack_iterator *iterator)
{
  delta_chain_link_t link;

  if (iterator->ptr >= iterator->end) {
    return NULL;
  }

  get_delta_chain_link_result_t next =
      getdeltachainlink(iterator->datapack->handle, iterator->ptr, &link);

  switch (next.code) {
  case GET_DELTA_CHAIN_LINK_OK:
    break;

  case GET_DELTA_CHAIN_LINK_OOM:
    PyErr_NoMemory();
    return NULL;

  case GET_DELTA_CHAIN_LINK_CORRUPT:
    PyErr_Format(PyExc_ValueError, "corruption in datapack");
    return NULL;
  }

  iterator->ptr = next.ptr;

  PyObject *tuple = NULL;
  PyObject *fn = NULL, *node = NULL, *deltabasenode = NULL, *deltalen = NULL;

  fn = PyString_FromStringAndSize(link.filename, link.filename_sz);
  node = PyString_FromStringAndSize((const char *)link.node, NODE_SZ);
  deltabasenode =
      PyString_FromStringAndSize((const char *)link.deltabase_node, NODE_SZ);
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
    PyObject_HEAD_INIT(NULL) 0,                    /* ob_size */
    "cdatapack.datapack.iterentries",              /* tp_name */
    sizeof(py_cdatapack_iterator),                 /* tp_basicsize */
    0,                                             /* tp_itemsize */
    (destructor)cdatapack_deltas_iterator_dealloc, /* tp_dealloc */
    0,                                             /* tp_print */
    0,                                             /* tp_getattr */
    0,                                             /* tp_setattr */
    0,                                             /* tp_compare */
    0,                                             /* tp_repr */
    0,                                             /* tp_as_number */
    0, /* tp_as_sequence - length/contains */
    0, /* tp_as_mapping - getitem/setitem*/
    0, /* tp_hash */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */

    Py_TPFLAGS_DEFAULT,                         /* tp_flags */
    "Iterator for delta chains in a datapack.", /* tp_doc */
    0,                                          /* tp_traverse */
    0,                                          /* tp_clear */
    0,                                          /* tp_richcompare */
    0,                                          /* tp_weaklistoffset */
    PyObject_SelfIter,                          /* tp_iter: __iter__() method */
    (iternextfunc)cdatapack_deltas_iterator_iternext, /* tp_iternext: next()
                                                       * method */
};

// ====  cdatapack_iterator class methods ====

/**
 * Deallocates a cdatapack iterator
 */
static void cdatapack_iterator_dealloc(py_cdatapack_iterator *self)
{
  Py_XDECREF(self->datapack);
  PyObject_Del(self);
}

/**
 * Yields the next item from the iterator.
 */
static PyObject *cdatapack_iterator_iternext(py_cdatapack_iterator *iterator)
{
  delta_chain_link_t link;

  if (iterator->ptr >= iterator->end) {
    return NULL;
  }

  get_delta_chain_link_result_t next =
      getdeltachainlink(iterator->datapack->handle, iterator->ptr, &link);

  switch (next.code) {
  case GET_DELTA_CHAIN_LINK_OK:
    break;

  case GET_DELTA_CHAIN_LINK_OOM:
    PyErr_NoMemory();
    return NULL;

  case GET_DELTA_CHAIN_LINK_CORRUPT:
    PyErr_Format(PyExc_ValueError, "corruption in datapack");
    return NULL;
  }

  iterator->ptr = next.ptr;

  PyObject *tuple = NULL, *fn = NULL, *node = NULL;

  fn = PyString_FromStringAndSize(link.filename, link.filename_sz);
  node = PyString_FromStringAndSize((const char *)link.node, NODE_SZ);
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
    PyObject_HEAD_INIT(NULL) 0,             /* ob_size */
    "cdatapack.datapack.iterator",          /* tp_name */
    sizeof(py_cdatapack_iterator),          /* tp_basicsize */
    0,                                      /* tp_itemsize */
    (destructor)cdatapack_iterator_dealloc, /* tp_dealloc */
    0,                                      /* tp_print */
    0,                                      /* tp_getattr */
    0,                                      /* tp_setattr */
    0,                                      /* tp_compare */
    0,                                      /* tp_repr */
    0,                                      /* tp_as_number */
    0, /* tp_as_sequence - length/contains */
    0, /* tp_as_mapping - getitem/setitem*/
    0, /* tp_hash */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */

    Py_TPFLAGS_DEFAULT,                           /* tp_flags */
    "Iterator for entries-tuples in a datapack.", /* tp_doc */
    0,                                            /* tp_traverse */
    0,                                            /* tp_clear */
    0,                                            /* tp_richcompare */
    0,                                            /* tp_weaklistoffset */
    PyObject_SelfIter,                         /* tp_iter: __iter__() method */
    (iternextfunc)cdatapack_iterator_iternext, /* tp_iternext: next() method */
};

/**
 * Initializes a cdatapack
 */
static int cdatapack_init(py_cdatapack *self, PyObject *args)
{
  self->handle = NULL;

  char *node;
  Py_ssize_t nodelen;

  if (!PyArg_ParseTuple(args, "s#", &node, &nodelen)) {
    return -1;
  }

  std::string idx_path(node);
  idx_path.append(INDEXSUFFIX);

  std::string data_path(node);
  data_path.append(PACKSUFFIX);

  self->handle = open_datapack(idx_path.data(), idx_path.size(),
                               data_path.data(), data_path.size());

  if (self->handle == NULL) {
    PyErr_NoMemory();
    return -1;
  } else if (self->handle->status == DATAPACK_HANDLE_OK) {
    return 0;
  }

  if (self->handle->status == DATAPACK_HANDLE_VERSION_MISMATCH) {
    PyErr_Format(PyExc_RuntimeError, "Unsupported version");
  } else if (self->handle->status != DATAPACK_HANDLE_OK) {
    PyErr_Format(PyExc_ValueError, "Error setting up datapack %s (status=%d)",
                 data_path.c_str(), self->handle->status);
  }

  free(self->handle);
  self->handle = NULL;
  return -1;
}

/**
 * Deallocates a cdatapack
 */
static void cdatapack_dealloc(py_cdatapack *self)
{
  if (self->handle != NULL) {
    close_datapack(self->handle);
  }
  PyObject_Del(self);
}

/**
 * Returns an iterator for a cdatapack.
 */
static py_cdatapack_iterator *cdatapack_getiter(py_cdatapack *self)
{
  py_cdatapack_iterator *iterator;

  iterator = PyObject_New(py_cdatapack_iterator, &cdatapack_iterator_type);
  if (iterator == NULL) {
    return NULL;
  }

  iterator->datapack = self;
  Py_INCREF(iterator->datapack);
  /* TODO: should have a data_version type and use sizeof(..) */
  iterator->ptr = ((uint8_t *)self->handle->data_mmap) + 1;
  iterator->end =
      ((uint8_t *)self->handle->data_mmap) + self->handle->data_file_sz;

  return iterator;
}

/**
 * Returns a delta iterator for a cdatapack.
 */
static py_cdatapack_iterator *cdatapack_getiterentries(py_cdatapack *self)
{
  py_cdatapack_iterator *iterator;

  iterator =
      PyObject_New(py_cdatapack_iterator, &cdatapack_deltas_iterator_type);
  if (iterator == NULL) {
    return NULL;
  }

  iterator->datapack = self;
  Py_INCREF(iterator->datapack);
  /* TODO: should have a data_version type and use sizeof(..) */
  iterator->ptr = ((uint8_t *)self->handle->data_mmap) + 1;
  iterator->end =
      ((uint8_t *)self->handle->data_mmap) + self->handle->data_file_sz;

  return iterator;
}

/**
 * Finds a node and returns a (node, deltabase index offset, data offset,
 * data size) tuple if found.
 */
static PyObject *cdatapack_find(py_cdatapack *self, PyObject *args)
{
  const char *node;
  Py_ssize_t node_sz;

  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }

  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  pack_index_entry_t pack_index_entry;

  if (find(self->handle, (const uint8_t *)node, &pack_index_entry) == false) {
    Py_RETURN_NONE;
  }

  PyObject *tuple = NULL;
  PyObject *retnode = NULL, *deltabaseindexoffset = NULL, *data_offset = NULL,
           *data_size = NULL;

  retnode =
      PyString_FromStringAndSize((const char *)pack_index_entry.node, NODE_SZ);
  deltabaseindexoffset =
      PyInt_FromLong(pack_index_entry.deltabase_index_offset);
  data_offset = PyLong_FromLongLong(pack_index_entry.data_offset);
  data_size = PyLong_FromLongLong(pack_index_entry.data_sz);

  if (retnode == NULL || deltabaseindexoffset == NULL || data_offset == NULL ||
      data_size == NULL) {
    goto cleanup;
  }
  tuple =
      PyTuple_Pack(4, retnode, deltabaseindexoffset, data_offset, data_size);

cleanup:

  Py_XDECREF(retnode);
  Py_XDECREF(deltabaseindexoffset);
  Py_XDECREF(data_offset);
  Py_XDECREF(data_size);

  return tuple;
}

PyObject *readpymeta(delta_chain_link_t *link)
{
  /* sync these with remotefilelog.constants */
  const char METAKEYFLAG = 'f';
  const char METAKEYSIZE = 's';

  PyObject *pymeta = PyDict_New();
  if (pymeta == NULL) {
    return PyErr_NoMemory();
  }

  if (link->meta == NULL || link->meta_sz == 0) {
    // no metadata, usually means it's version 0
    return pymeta;
  }

  const char *p = (const char *)link->meta;
  const char *end = p + link->meta_sz;

  while (p + 3 <= end) { /* 3: ensure 1-byte key, 2-byte size exist */
    const char key[2] = {*p, 0};
    p += 1;

    const uint16_t entry_size = ntohs(*((uint16_t *)p));
    p += sizeof(entry_size); /* 2-byte size */

    if (entry_size + p > end) {
      goto err_cleanup;
    }

    PyObject *pyv = NULL;
    switch (key[0]) {
    case METAKEYFLAG:
    case METAKEYSIZE: { /* an integer field */
      unsigned long long v = 0;
      for (const char *vp = p; vp < p + entry_size; ++vp) {
        v = (v << 8) | *((uint8_t *)vp);
      }
      pyv = PyLong_FromUnsignedLongLong(v);
    } break;
    default: { /* treat value as a string field */
      pyv = PyString_FromStringAndSize(p, entry_size);
    }
    }
    if (pyv == NULL) {
      goto err_cleanup;
    }
    if (PyDict_SetItemString(pymeta, key, pyv) == -1) {
      Py_XDECREF(pyv);
      goto err_cleanup;
    }
    p += entry_size;
  }
  if (p != end) {
    goto err_cleanup;
  }

  return pymeta;

err_cleanup:
  PyErr_Format(PyExc_ValueError, "corrupted datapack metadata");
  Py_XDECREF(pymeta);
  return NULL;
}

/**
 *  Finds a node and returns its delta entry (delta, deltabasenode,
 *  meta) tuple if found.
 */
static PyObject *cdatapack_getdelta(py_cdatapack *self, PyObject *args)
{
  const char *node;
  Py_ssize_t node_sz;

  // 1. Parse the args
  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }

  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  // 2. Read the delta chain
  pack_index_entry_t index_entry;

  if (find(self->handle, (const uint8_t *)node, &index_entry) == false) {
    PyErr_SetObject(PyExc_KeyError, PyTuple_GET_ITEM(args, 0));
    return NULL;
  }

  delta_chain_link_t link;

  get_delta_chain_link_result_t next = getdeltachainlink(
      self->handle,
      ((uint8_t *)self->handle->data_mmap) + index_entry.data_offset, &link);

  if (next.code != GET_DELTA_CHAIN_LINK_OK) {
    PyErr_SetObject(PyExc_KeyError, PyTuple_GET_ITEM(args, 0));
    return NULL;
  }

  // Populate the link.delta pointer
  if (!uncompressdeltachainlink(&link)) {
    PyErr_Format(PyExc_ValueError, "unable to decompress pack entry");
    return NULL;
  }

  // 3. Convert it into python objects
  PyObject *tuple = NULL;
  PyObject *delta = NULL, *deltabasenode = NULL, *meta = NULL;

  delta = PyBytes_FromStringAndSize((const char *)link.delta,
                                    (Py_ssize_t)link.delta_sz);
  deltabasenode =
      PyBytes_FromStringAndSize((const char *)link.deltabase_node, NODE_SZ);
  meta = readpymeta(&link);

  if (deltabasenode != NULL && delta != NULL && meta != NULL) {
    tuple = PyTuple_Pack(3, delta, deltabasenode, meta);
  }

  Py_XDECREF(delta);
  Py_XDECREF(deltabasenode);
  Py_XDECREF(meta);

  if (tuple == NULL) {
    goto err_cleanup;
  }

  goto cleanup;

err_cleanup:
  Py_XDECREF(tuple);
  tuple = NULL;

cleanup:
  free((void *)link.delta);
  return tuple;
}

/**
 * Finds a node and returns a list of (filename, node, filename, delta base
 * node, delta) tuples if found.
 */
static PyObject *cdatapack_getdeltachain(py_cdatapack *self, PyObject *args)
{
  const char *node;
  Py_ssize_t node_sz;

  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }

  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  delta_chain_t chain = getdeltachain(self->handle, (const uint8_t *)node);
  if (chain.code == GET_DELTA_CHAIN_OOM) {
    PyErr_NoMemory();
    return NULL;
  } else if (chain.code == GET_DELTA_CHAIN_NOT_FOUND) {
    Py_RETURN_NONE;
  } else if (chain.code != GET_DELTA_CHAIN_OK) {
    // corrupt, etc.
    PyErr_Format(PyExc_ValueError, "unknown error reading node %s", node);
    return NULL;
  }
  PyObject *result = PyList_New(chain.links_count);
  if (result == NULL) {
    goto err_cleanup;
  }

  for (size_t ix = 0; ix < chain.links_count; ix++) {
    PyObject *tuple = NULL;
    PyObject *name = NULL, *retnode = NULL, *deltabasenode = NULL,
             *delta = NULL;

    delta_chain_link_t *link = &chain.delta_chain_links[ix];

    name = PyString_FromStringAndSize(link->filename, link->filename_sz);
    retnode = PyString_FromStringAndSize((const char *)link->node, NODE_SZ);
    deltabasenode =
        PyString_FromStringAndSize((const char *)link->deltabase_node, NODE_SZ);
    delta = PyString_FromStringAndSize((const char *)link->delta,
                                       (Py_ssize_t)link->delta_sz);

    if (name != NULL && retnode != NULL && deltabasenode != NULL &&
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

static PyObject *cdatapack_getmeta(py_cdatapack *self, PyObject *args)
{
  const char *node;
  Py_ssize_t node_sz;

  if (!PyArg_ParseTuple(args, "s#", &node, &node_sz)) {
    return NULL;
  }
  if (node_sz != NODE_SZ) {
    PyErr_Format(PyExc_ValueError, "node must be %d bytes long", NODE_SZ);
    return NULL;
  }

  pack_index_entry_t index_entry;

  if (find(self->handle, (const uint8_t *)node, &index_entry) == false) {
    PyErr_SetObject(PyExc_KeyError, &args[0]);
    return NULL;
  }

  delta_chain_link_t link;

  get_delta_chain_link_result_t next = getdeltachainlink(
      self->handle,
      ((uint8_t *)self->handle->data_mmap) + index_entry.data_offset, &link);

  if (next.code != GET_DELTA_CHAIN_LINK_OK) {
    PyErr_SetObject(PyExc_KeyError, &args[0]);
    return NULL;
  }

  return readpymeta(&link);
}

// ====  cdatapack ctype declaration ====

static PyMethodDef cdatapack_methods[] = {
    {"iterentries", (PyCFunction)cdatapack_getiterentries, METH_NOARGS,
     "Iterate over (path, nodeid, deltabasenode, delta) tuples in this "
     "datapack."},
    {"_find", (PyCFunction)cdatapack_find, METH_VARARGS,
     "Finds a node and returns a (node, deltabase index offset, "
     "data offset, data size) tuple if found."},
    {"getdelta", (PyCFunction)cdatapack_getdelta, METH_VARARGS,
     "Finds a node and returns its delta entry (delta, deltabasename, "
     "deltabasenode, meta) tuple if found."},
    {"getdeltachain", (PyCFunction)cdatapack_getdeltachain, METH_VARARGS,
     "Finds a node and returns a list of (filename, node, filename, delta "
     "base node, delta) tuples if found."},
    {"getmeta", (PyCFunction)cdatapack_getmeta, METH_VARARGS,
     "Return a metadata dictionary for given node"},
    {NULL, NULL}};

static PyTypeObject cdatapack_type = {
    PyObject_HEAD_INIT(NULL) 0,     /* ob_size */
    "cdatapack.datapack",           /* tp_name */
    sizeof(py_cdatapack),           /* tp_basicsize */
    0,                              /* tp_itemsize */
    (destructor)cdatapack_dealloc,  /* tp_dealloc */
    0,                              /* tp_print */
    0,                              /* tp_getattr */
    0,                              /* tp_setattr */
    0,                              /* tp_compare */
    0,                              /* tp_repr */
    0,                              /* tp_as_number */
    0,                              /* tp_as_sequence - length/contains */
    0,                              /* tp_as_mapping - getitem/setitem*/
    0,                              /* tp_hash */
    0,                              /* tp_call */
    0,                              /* tp_str */
    0,                              /* tp_getattro */
    0,                              /* tp_setattro */
    0,                              /* tp_as_buffer */
    Py_TPFLAGS_DEFAULT,             /* tp_flags */
    "TODO",                         /* tp_doc */
    0,                              /* tp_traverse */
    0,                              /* tp_clear */
    0,                              /* tp_richcompare */
    0,                              /* tp_weaklistoffset */
    (getiterfunc)cdatapack_getiter, /* tp_iter */
    0,                              /* tp_iternext */
    cdatapack_methods,              /* tp_methods */
    0,                              /* tp_members */
    0,                              /* tp_getset */
    0,                              /* tp_base */
    0,                              /* tp_dict */
    0,                              /* tp_descr_get */
    0,                              /* tp_descr_set */
    0,                              /* tp_dictoffset */
    (initproc)cdatapack_init,       /* tp_init */
    0,                              /* tp_alloc */
};

#endif /* FBHGEXT_CSTORE_PY_CDATAPACK_H */
