// Copyright 2016-present Facebook. All Rights Reserved.
//
// fastmanifest_wrapper.c: CPython interface for fastmanifest
//
// no-check-code

#include <Python.h>

#if defined(_MSC_VER) || __STDC_VERSION__ < 199901L
#define true 1
#define false 0
typedef unsigned char bool;
#else
#include <stdbool.h>
#endif

#include "tree.h"

typedef struct {
  PyObject_HEAD;
  tree_t *tree;
} fastmanifest;


static PyTypeObject fastmanifestType;

/* ========================== */
/* Fastmanifest: C Interface */
/* ========================== */

/* Deallocate all the nodes in the tree */
static void ifastmanifest_dealloc(fastmanifest *self)
{
  destroy_tree(self->tree);
}

static fastmanifest *ifastmanifest_copy(fastmanifest *copy, fastmanifest *self)
{
  copy->tree = copy_tree(self->tree);
  return copy;
}

static void ifastmanifest_save(fastmanifest *copy, char *filename, size_t len)
{
  /* TODO integration with @ttung */
}

static void ifastmanifest_load(fastmanifest *copy, char *filename, size_t len)
{
  /* TODO integration with @ttung */
}

static get_path_result_t ifastmanifest_getitem(
    fastmanifest *self, char *path, ssize_t plen)
{
  get_path_result_t get_path_result = get_path(self->tree, path, plen);
  return get_path_result;
}

static add_update_path_result_t ifastmanifest_insert(
    fastmanifest *self,
    char *path, ssize_t plen,
    unsigned char *hash, ssize_t hlen,
    char *flags, ssize_t flen)
{
  add_update_path_result_t result = add_or_update_path(
      self->tree,
      path, plen,
      hash, hlen,
      *flags);

  return result;
}


static convert_from_flat_code_t ifastmanifest_init(
    fastmanifest *self, char *data, ssize_t len)
{
  convert_from_flat_result_t from_result = convert_from_flat(
      data, len);

  if (from_result.code == CONVERT_FROM_FLAT_OK) {
    tree_t *tree = from_result.tree;
    self->tree = tree;
  }

  return from_result.code;
}

static ssize_t ifastmanifest_size(fastmanifest *self)
{
  return self->tree->num_leaf_nodes;
}

static remove_path_result_t ifastmanifest_delitem(
    fastmanifest *self, char *path, size_t plen)
{
  remove_path_result_t remove_path_result =
      remove_path(self->tree, path, plen);
  return remove_path_result;
}

/* Fastmanifest: end of pure C layer | start of CPython layer */

/* Fastmanifest: CPython helpers */

static bool fastmanifest_is_valid_manifest_key(PyObject *key) {
  if (PyString_Check(key)) {
    return true;
  } else {
    PyErr_Format(PyExc_TypeError, "Manifest keys must be strings.");
    return false;
  }
}

static bool fastmanifest_is_valid_manifest_value(PyObject *value) {
  if (!PyTuple_Check(value) || PyTuple_Size(value) != 2) {
    PyErr_Format(PyExc_TypeError,
           "Manifest values must be a tuple of (node, flags).");
    return false;
  }
  return true;
}

static PyObject *fastmanifest_formatfile(
    const uint8_t *checksum, const uint8_t checksum_sz, const uint8_t flags) {
  PyObject *py_checksum = PyString_FromStringAndSize(
      (const char *) checksum, checksum_sz);

  if (!py_checksum) {
    return NULL;
  }

  PyObject *py_flags;
  PyObject *tup;

  py_flags = PyString_FromStringAndSize(
      (const char *) &flags, (flags == 0) ? 0 : 1);
  if (!py_flags) {
    Py_DECREF(py_checksum);
    return NULL;
  }
  tup = PyTuple_Pack(2, py_checksum, py_flags);
  Py_DECREF(py_flags);
  Py_DECREF(py_checksum);
  return tup;
}

/* ================================== */
/* Fastmanifest: CPython Interface */
/* ================================== */

static int fastmanifest_init(fastmanifest *self, PyObject *args) {
  PyObject *pydata = NULL;
  char *data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return -1;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1)
    return -1;
  convert_from_flat_code_t from_code = ifastmanifest_init(self, data, len);
  switch(from_code) {
  case CONVERT_FROM_FLAT_OOM:
    PyErr_NoMemory();
    return -1;
  case CONVERT_FROM_FLAT_WTF:
    PyErr_Format(PyExc_ValueError,
           "Manifest did not end in a newline.");
    return -1;
  default:
    return 0;
  }
}

static void fastmanifest_dealloc(fastmanifest *self) {
  ifastmanifest_dealloc(self);
}

static PyObject *fastmanifest_getkeysiter(fastmanifest *self) {
  return NULL;
}

static PyObject * fastmanifest_save(fastmanifest *self, PyObject *args){
  PyObject *pydata = NULL;
  char *data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return NULL;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1 || len < 0)
    return NULL;
  /* TODO @ttung error handling */
  ifastmanifest_save(self, data, (size_t) len);
	return NULL;
}

static PyObject *fastmanifest_load(fastmanifest *self, PyObject *args) {
  PyObject *pydata = NULL;
  char *data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return NULL;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1 || len < 0)
    return NULL;
  /* TODO @ttung error handling */
  ifastmanifest_load(self, data, (size_t) len);
	return NULL;
}

static fastmanifest *fastmanifest_copy(fastmanifest *self)
{

  fastmanifest *copy = PyObject_New(fastmanifest, &fastmanifestType);
  if (copy)
    ifastmanifest_copy(copy, self);

  if (!copy)
    PyErr_NoMemory();
  return copy;
}

static Py_ssize_t fastmanifest_size(fastmanifest *self)
{
  return ifastmanifest_size(self);
}

static PyObject *fastmanifest_getitem(fastmanifest *self, PyObject *key)
{

  if (!fastmanifest_is_valid_manifest_key(key)) {
    return NULL;
  }

  char *ckey;
  ssize_t clen;
  int err = PyString_AsStringAndSize(key, &ckey, &clen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding path");
    return NULL;
  }

  get_path_result_t query = ifastmanifest_getitem(self, ckey, clen);
  switch (query.code) {
  case GET_PATH_NOT_FOUND:
    PyErr_Format(PyExc_KeyError,
           "File not found");
    return NULL;

  case GET_PATH_WTF:
    PyErr_Format(PyExc_ValueError,
           "tree corrupt");
    return NULL;

  default:
    break;
  }

  PyObject *ret = fastmanifest_formatfile(
      query.checksum, query.checksum_sz, query.flags);
  if (ret == NULL) {
    PyErr_Format(PyExc_ValueError,
           "Error formatting file");
  }
  return ret;
}

static int fastmanifest_setitem(fastmanifest *self, PyObject *key,
                PyObject *value)
{
  char *path, *hash, *flags;
  ssize_t plen, hlen, flen;
  int err;
  /* Decode path */
  if (!fastmanifest_is_valid_manifest_key(key)) {
    return -1;
  }
  err = PyString_AsStringAndSize(key, &path, &plen);
  if (err == -1 || plen < 0) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding path");
    return -1;
  }

  if (!value) {
    remove_path_result_t remove_path_result =
        ifastmanifest_delitem(self, path, (size_t) plen);

   switch(remove_path_result) {

    case REMOVE_PATH_OK:
      return 0;

    case REMOVE_PATH_NOT_FOUND:
      PyErr_Format(PyExc_KeyError,
             "Not found");
      return -1;

    case REMOVE_PATH_WTF:
      PyErr_Format(PyExc_KeyError,
             "tree corrupt");
      return -1;
    }
  }

  /* Decode node and flags*/
  if (!fastmanifest_is_valid_manifest_value(value)) {
    return -1;
  }
  PyObject *pyhash = PyTuple_GetItem(value, 0);

  err = PyString_AsStringAndSize(pyhash, &hash, &hlen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding hash");
    return -1;
  }

  PyObject *pyflags = PyTuple_GetItem(value, 1);

  err = PyString_AsStringAndSize(pyflags, &flags, &flen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding flags");
    return -1;
  }

  add_update_path_result_t add_update_path_result =
      ifastmanifest_insert(self, path, plen,
          (unsigned char *) hash, hlen, flags, flen);
  switch (add_update_path_result) {
    case ADD_UPDATE_PATH_OOM:
    {
      PyErr_NoMemory();
      return -1;
    }

    case ADD_UPDATE_PATH_OK:
      return 0;

    default:
    {
      PyErr_Format(PyExc_TypeError,
           "unexpected stuff happened");
      return -1;
    }
  }
}

static PyMappingMethods fastmanifest_mapping_methods = {
  (lenfunc)fastmanifest_size,          /* mp_length */
  (binaryfunc)fastmanifest_getitem,    /* mp_subscript */
  (objobjargproc)fastmanifest_setitem, /* mp_ass_subscript */
};

/* sequence methods (important or __contains__ builds an iterator) */

static int fastmanifest_contains(fastmanifest *self, PyObject *key)
{
  if (!fastmanifest_is_valid_manifest_key(key)) {
    /* Our keys are always strings, so if the contains
     * check is for a non-string, just return false. */
    return 0;
  }
  char *path;
  ssize_t plen;
  int err = PyString_AsStringAndSize(key, &path, &plen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding path");
    return -1;
  }
  return contains_path(self->tree, path, plen) ? 1 : 0;
}

static PySequenceMethods fastmanifest_seq_meths = {
  (lenfunc)fastmanifest_size, /* sq_length */
  0, /* sq_concat */
  0, /* sq_repeat */
  0, /* sq_item */
  0, /* sq_slice */
  0, /* sq_ass_item */
  0, /* sq_ass_slice */
  (objobjproc)fastmanifest_contains, /* sq_contains */
  0, /* sq_inplace_concat */
  0, /* sq_inplace_repeat */
};

static PyMethodDef fastmanifest_methods[] = {
  {"iterkeys", (PyCFunction)fastmanifest_getkeysiter, METH_NOARGS,
   "Iterate over file names in this fastmanifest."},
  {"copy", (PyCFunction)fastmanifest_copy, METH_NOARGS,
   "Make a copy of this fastmanifest."},
  {"save", (PyCFunction)fastmanifest_save, METH_NOARGS,
   "Save a fastmanifest to a file"},
  {"load", (PyCFunction)fastmanifest_load, METH_NOARGS,
   "Load a tree manifest from a file"},
  {NULL},
};

static PyTypeObject fastmanifestType = {
  PyObject_HEAD_INIT(NULL)
  0,                                                /* ob_size */
  "parsers.fastmanifest",                           /* tp_name */
  sizeof(fastmanifest),                             /* tp_basicsize */
  0,                                                /* tp_itemsize */
  (destructor)fastmanifest_dealloc,                 /* tp_dealloc */
  0,                                                /* tp_print */
  0,                                                /* tp_getattr */
  0,                                                /* tp_setattr */
  0,                                                /* tp_compare */
  0,                                                /* tp_repr */
  0,                                                /* tp_as_number */
  &fastmanifest_seq_meths,                          /* tp_as_sequence */
  &fastmanifest_mapping_methods,                    /* tp_as_mapping */
  0,                                                /* tp_hash */
  0,                                                /* tp_call */
  0,                                                /* tp_str */
  0,                                                /* tp_getattro */
  0,                                                /* tp_setattro */
  0,                                                /* tp_as_buffer */
  Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_SEQUENCE_IN, /* tp_flags */
  "TODO(augie)",                                    /* tp_doc */
  0,                                                /* tp_traverse */
  0,                                                /* tp_clear */
  0,                                                /* tp_richcompare */
  0,                                             /* tp_weaklistoffset */
  (getiterfunc)fastmanifest_getkeysiter,                /* tp_iter */
  0,                                                /* tp_iternext */
  fastmanifest_methods,                             /* tp_methods */
  0,                                                /* tp_members */
  0,                                                /* tp_getset */
  0,                                                /* tp_base */
  0,                                                /* tp_dict */
  0,                                                /* tp_descr_get */
  0,                                                /* tp_descr_set */
  0,                                                /* tp_dictoffset */
  (initproc)fastmanifest_init,                      /* tp_init */
  0,                                                /* tp_alloc */
};

static PyMethodDef methods[] = {
  {NULL, NULL, 0, NULL}
};

PyMODINIT_FUNC
initfastmanifest_wrapper(void)
{
    PyObject* m;

    fastmanifestType.tp_new = PyType_GenericNew;
    if (PyType_Ready(&fastmanifestType) < 0)
        return;

    m = Py_InitModule3("fastmanifest_wrapper", methods,
                       "Wrapper around fast_manifest");

    Py_INCREF(&fastmanifestType);
    PyModule_AddObject(m, "fastManifest", (PyObject *)&fastmanifestType);
}
