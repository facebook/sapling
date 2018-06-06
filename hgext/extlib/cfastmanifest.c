// Copyright 2016-present Facebook. All Rights Reserved.
//
// cfastmanifest.c: CPython interface for fastmanifest
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

#include "hgext/extlib/cfastmanifest/tree.h"

// clang-format off
// clang thinks that PyObject_HEAD should be on the same line as the next line
// since there is no semicolong after it. There is no semicolon because
// PyObject_HEAD macro already contains one and MSVC does not support
// extra semicolons.
typedef struct {
  PyObject_HEAD
  tree_t *tree;
} fastmanifest;
// clang-format on

// clang-format off
typedef struct {
  PyObject_HEAD
  iterator_t *iterator;
} fmIter;
// clang-format on

static PyTypeObject fastmanifestType;
static PyTypeObject fastmanifestKeysIterator;
static PyTypeObject fastmanifestEntriesIterator;

/* Fastmanifest: CPython helpers */

static bool fastmanifest_is_valid_manifest_key(PyObject* key) {
  return PyString_Check(key);
}

static bool fastmanifest_is_valid_manifest_value(PyObject* value) {
  if (!PyTuple_Check(value) || PyTuple_Size(value) != 2) {
    PyErr_Format(
        PyExc_TypeError, "Manifest values must be a tuple of (node, flags).");
    return false;
  }
  return true;
}

static PyObject* fastmanifest_formatfile(
    const uint8_t* checksum,
    const uint8_t checksum_sz,
    const uint8_t flags) {
  PyObject* py_checksum =
      PyString_FromStringAndSize((const char*)checksum, checksum_sz);

  if (!py_checksum) {
    return NULL;
  }

  PyObject* py_flags;
  PyObject* tup;

  py_flags =
      PyString_FromStringAndSize((const char*)&flags, (flags == 0) ? 0 : 1);
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

static int fastmanifest_init(fastmanifest* self, PyObject* args) {
  PyObject* pydata = NULL;
  char* data;
  ssize_t len;

  if (!PyArg_ParseTuple(args, "|S", &pydata)) {
    return -1;
  }

  if (pydata == NULL) {
    // no string.  initialize it to an empty tree.
    self->tree = alloc_tree();
    if (self->tree == NULL) {
      PyErr_NoMemory();
      return -1;
    }
    return 0;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1)
    return -1;

  convert_from_flat_result_t from_result = convert_from_flat(data, len);

  if (from_result.code == CONVERT_FROM_FLAT_OK) {
    tree_t* tree = from_result.tree;
    self->tree = tree;
  } else {
    self->tree = NULL;
  }

  switch (from_result.code) {
    case CONVERT_FROM_FLAT_OOM:
      PyErr_NoMemory();
      return -1;
    case CONVERT_FROM_FLAT_WTF:
      PyErr_Format(PyExc_ValueError, "Manifest did not end in a newline.");
      return -1;
    default:
      return 0;
  }
}

static void fastmanifest_dealloc(fastmanifest* self) {
  destroy_tree(self->tree);
  PyObject_Del(self);
}

static PyObject* fastmanifest_getkeysiter(fastmanifest* self) {
  fmIter* i = NULL;
  iterator_t* iterator = create_iterator(self->tree, true);
  if (!iterator) {
    PyErr_NoMemory();
    return NULL;
  }
  i = PyObject_New(fmIter, &fastmanifestKeysIterator);
  if (i) {
    i->iterator = iterator;
  } else {
    destroy_iterator(iterator);
    PyErr_NoMemory();
  }
  return (PyObject*)i;
}

static PyObject* fastmanifest_getentriesiter(fastmanifest* self) {
  fmIter* i = NULL;
  iterator_t* iterator = create_iterator(self->tree, true);
  if (!iterator) {
    PyErr_NoMemory();
    return NULL;
  }
  i = PyObject_New(fmIter, &fastmanifestEntriesIterator);
  if (i) {
    i->iterator = iterator;
  } else {
    destroy_iterator(iterator);
    PyErr_NoMemory();
  }
  return (PyObject*)i;
}

static PyObject* fastmanifest_save(fastmanifest* self, PyObject* args) {
  PyObject* pydata = NULL;
  char* data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return NULL;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1 || len < 0) {
    PyErr_Format(PyExc_ValueError, "Illegal filepath");
    return NULL;
  }
  write_to_file_result_t result = write_to_file(self->tree, data, (size_t)len);

  switch (result) {
    case WRITE_TO_FILE_OK:
      Py_RETURN_NONE;

    case WRITE_TO_FILE_OOM:
      PyErr_NoMemory();
      return NULL;

    default:
      PyErr_Format(PyExc_ValueError, "Unexpected error saving manifest");
      return NULL;
  }
}

static PyObject* fastmanifest_load(PyObject* cls, PyObject* args) {
  PyObject* pydata = NULL;
  char* data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return NULL;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1 || len < 0) {
    PyErr_Format(PyExc_ValueError, "Illegal filepath");
    return NULL;
  }
  read_from_file_result_t result = read_from_file(data, (size_t)len);

  switch (result.code) {
    case READ_FROM_FILE_OK: {
      fastmanifest* read_manifest =
          PyObject_New(fastmanifest, &fastmanifestType);
      read_manifest->tree = result.tree;
      return (PyObject*)read_manifest;
    }

    case READ_FROM_FILE_OOM:
      PyErr_NoMemory();
      return NULL;

    case READ_FROM_FILE_NOT_READABLE:
      errno = result.err;
      PyErr_SetFromErrno(PyExc_IOError);
      return NULL;

    default:
      PyErr_Format(PyExc_ValueError, "Unexpected error loading manifest");
      return NULL;
  }
}

static fastmanifest* fastmanifest_copy(fastmanifest* self) {
  fastmanifest* copy = PyObject_New(fastmanifest, &fastmanifestType);
  if (copy) {
    copy->tree = copy_tree(self->tree);
  }

  if (!copy)
    PyErr_NoMemory();
  return copy;
}

typedef struct {
  PyObject* matchfn;
  bool filter_error_occurred;
} filter_copy_context_t;

bool filter_callback(char* path, size_t path_sz, void* callback_context) {
  filter_copy_context_t* context = (filter_copy_context_t*)callback_context;

  PyObject *arglist = NULL, *result = NULL;
  arglist = Py_BuildValue("(s#)", path, (int)path_sz);
  if (!arglist) {
    context->filter_error_occurred = true;
    return false;
  }

  result = PyObject_CallObject(context->matchfn, arglist);
  Py_DECREF(arglist);
  if (!result) {
    context->filter_error_occurred = true;
    return false;
  }

  bool bool_result = PyObject_IsTrue(result);
  Py_DECREF(result);
  return bool_result;
}

static fastmanifest* fastmanifest_filtercopy(
    fastmanifest* self,
    PyObject* matchfn) {
  fastmanifest* py_copy = PyObject_New(fastmanifest, &fastmanifestType);
  tree_t* copy = NULL;
  if (py_copy) {
    filter_copy_context_t context;

    context.matchfn = matchfn;
    context.filter_error_occurred = false;

    copy = filter_copy(self->tree, filter_callback, &context);

    if (copy == NULL) {
      goto cleanup;
    }

    py_copy->tree = copy;
    return py_copy;
  }

cleanup:
  if (copy != NULL) {
    destroy_tree(copy);
  }

  if (py_copy != NULL) {
    Py_DECREF(py_copy);
  }

  PyErr_NoMemory();

  return NULL;
}

static PyObject* hashflags(
    const uint8_t* checksum,
    const uint8_t checksum_sz,
    const uint8_t flags) {
  PyObject *ret = NULL, *py_hash, *py_flags;
  py_hash = PyString_FromStringAndSize((const char*)checksum, checksum_sz);
  py_flags =
      PyString_FromStringAndSize((const char*)&flags, flags == 0 ? 0 : 1);
  if (!py_hash || !py_flags) {
    goto cleanup;
  }
  ret = PyTuple_Pack(2, py_hash, py_flags);

cleanup:
  Py_XDECREF(py_hash);
  Py_XDECREF(py_flags);
  return ret;
}

typedef struct _fastmanifest_diff_context_t {
  PyObject* result;
  PyObject* emptyTuple;
  bool error_occurred;
  bool listclean;
} fastmanifest_diff_context_t;

static void fastmanifest_diff_callback(
    const char* path,
    const size_t path_sz,
    const bool left_present,
    const uint8_t* left_checksum,
    const uint8_t left_checksum_sz,
    const uint8_t left_flags,
    const bool right_present,
    const uint8_t* right_checksum,
    const uint8_t right_checksum_sz,
    const uint8_t right_flags,
    void* context) {
  fastmanifest_diff_context_t* diff_context =
      (fastmanifest_diff_context_t*)context;
  PyObject *key, *outer = NULL, *py_left = NULL, *py_right = NULL;

  key = PyString_FromStringAndSize(path, path_sz);
  if (!key) {
    diff_context->error_occurred = true;
    goto cleanup;
  }

  if (left_present && right_present && left_flags == right_flags &&
      left_checksum_sz == right_checksum_sz &&
      memcmp(left_checksum, right_checksum, left_checksum_sz) == 0) {
    Py_INCREF(Py_None);
    outer = Py_None;
  } else {
    if (left_present) {
      py_left = hashflags(left_checksum, left_checksum_sz, left_flags);
    } else {
      py_left = diff_context->emptyTuple;
    }

    if (right_present) {
      py_right = hashflags(right_checksum, right_checksum_sz, right_flags);
    } else {
      py_right = diff_context->emptyTuple;
    }

    if (!py_left || !py_right) {
      diff_context->error_occurred = true;
      goto cleanup;
    }

    outer = PyTuple_Pack(2, py_left, py_right);
    if (outer == NULL) {
      diff_context->error_occurred = true;
      goto cleanup;
    }
  }

  if (PyDict_SetItem(diff_context->result, key, outer) != 0) {
    diff_context->error_occurred = true;
  }

cleanup:
  Py_XDECREF(outer);
  Py_XDECREF(key);
  if (left_present) {
    Py_XDECREF(py_left);
  }
  if (right_present) {
    Py_XDECREF(py_right);
  }
}

static PyObject*
fastmanifest_diff(fastmanifest* self, PyObject* args, PyObject* kwargs) {
  fastmanifest* other;
  PyObject *match = NULL, *pyclean = NULL;
  PyObject *emptyTuple = NULL, *ret = NULL;
  PyObject* es;
  fastmanifest_diff_context_t context;
  context.error_occurred = false;

  static char const* kwlist[] = {"m2", "match", "clean", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args,
          kwargs,
          "O!|OO",
          (char**)kwlist,
          &fastmanifestType,
          &other,
          &match,
          &pyclean)) {
    return NULL;
  }

  if (match && match != Py_None) {
    PyErr_Format(
        PyExc_ValueError,
        "fastmanifest.diff does not support the match argument");
    return NULL;
  }

  context.listclean = (!pyclean) ? false : PyObject_IsTrue(pyclean);
  es = PyString_FromString("");
  if (!es) {
    goto nomem;
  }
  emptyTuple = PyTuple_Pack(2, Py_None, es);
  Py_CLEAR(es);
  if (!emptyTuple) {
    goto nomem;
  }
  ret = PyDict_New();
  if (!ret) {
    goto nomem;
  }

  context.result = ret;
  context.emptyTuple = emptyTuple;

  diff_result_t diff_result = diff_trees(
      self->tree,
      other->tree,
      context.listclean,
      &fastmanifest_diff_callback,
      &context);

  Py_CLEAR(emptyTuple);

  switch (diff_result) {
    case DIFF_OK:
      if (context.error_occurred) {
        // error occurred in the callback, i.e., our code.
        Py_XDECREF(ret);
        if (PyErr_Occurred() == NULL) {
          PyErr_Format(
              PyExc_ValueError,
              "ignore_fastmanifest_errcode set but no exception detected.");
        }
        return NULL;
      }

      return ret;

    case DIFF_OOM:
      goto nomem;

    case DIFF_WTF:
      PyErr_Format(PyExc_ValueError, "Unexpected error diffing manifests.");
      goto cleanup;
  }

nomem:
  PyErr_NoMemory();

cleanup:
  Py_XDECREF(ret);
  Py_XDECREF(emptyTuple);
  Py_XDECREF(es);
  return NULL;
}

static PyObject* fastmanifest_text(fastmanifest* self) {
  convert_to_flat_result_t to_flat = convert_to_flat(self->tree);
  switch (to_flat.code) {
    case CONVERT_TO_FLAT_OK:
      return PyString_FromStringAndSize(
          to_flat.flat_manifest, to_flat.flat_manifest_sz);

    case CONVERT_TO_FLAT_OOM:
      PyErr_NoMemory();
      return NULL;

    case CONVERT_TO_FLAT_WTF:
      PyErr_Format(PyExc_ValueError, "Error converting manifest");
      return NULL;

    default:
      PyErr_Format(PyExc_ValueError, "Unknown result code");
      return NULL;
  }
}

static Py_ssize_t fastmanifest_size(fastmanifest* self) {
  return self->tree->num_leaf_nodes;
}

static PyObject* fastmanifest_bytes(fastmanifest* self) {
  return PyInt_FromSize_t(self->tree->consumed_memory);
}

static PyObject* fastmanifest_getitem(fastmanifest* self, PyObject* key) {
  if (!fastmanifest_is_valid_manifest_key(key)) {
    PyErr_Format(PyExc_TypeError, "Manifest keys must be strings.");
    return NULL;
  }

  char* ckey;
  ssize_t clen;
  int err = PyString_AsStringAndSize(key, &ckey, &clen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError, "Error decoding path");
    return NULL;
  }

  get_path_result_t query = get_path(self->tree, ckey, clen);
  switch (query.code) {
    case GET_PATH_NOT_FOUND:
      PyErr_Format(PyExc_KeyError, "File not found");
      return NULL;

    case GET_PATH_WTF:
      PyErr_Format(PyExc_ValueError, "tree corrupt");
      return NULL;

    default:
      break;
  }

  PyObject* ret =
      fastmanifest_formatfile(query.checksum, query.checksum_sz, query.flags);
  if (ret == NULL) {
    PyErr_Format(PyExc_ValueError, "Error formatting file");
  }
  return ret;
}

static int
fastmanifest_setitem(fastmanifest* self, PyObject* key, PyObject* value) {
  char *path, *hash, *flags;
  ssize_t plen, hlen, flen;
  int err;
  /* Decode path */
  if (!fastmanifest_is_valid_manifest_key(key)) {
    PyErr_Format(PyExc_TypeError, "Manifest keys must be strings.");
    return -1;
  }
  err = PyString_AsStringAndSize(key, &path, &plen);
  if (err == -1 || plen < 0) {
    PyErr_Format(PyExc_TypeError, "Error decoding path");
    return -1;
  }

  if (!value) {
    remove_path_result_t remove_path_result =
        remove_path(self->tree, path, (size_t)plen);

    switch (remove_path_result) {
      case REMOVE_PATH_OK:
        return 0;

      case REMOVE_PATH_NOT_FOUND:
        PyErr_Format(PyExc_KeyError, "Not found");
        return -1;

      case REMOVE_PATH_WTF:
        PyErr_Format(PyExc_KeyError, "tree corrupt");
        return -1;
    }
  }

  /* Decode node and flags*/
  if (!fastmanifest_is_valid_manifest_value(value)) {
    return -1;
  }
  PyObject* pyhash = PyTuple_GetItem(value, 0);

  err = PyString_AsStringAndSize(pyhash, &hash, &hlen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError, "Error decoding hash");
    return -1;
  }

  PyObject* pyflags = PyTuple_GetItem(value, 1);

  err = PyString_AsStringAndSize(pyflags, &flags, &flen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError, "Error decoding flags");
    return -1;
  }

  add_update_path_result_t add_update_path_result = add_or_update_path(
      self->tree, path, plen, (unsigned char*)hash, hlen, *flags);
  switch (add_update_path_result) {
    case ADD_UPDATE_PATH_OOM: {
      PyErr_NoMemory();
      return -1;
    }

    case ADD_UPDATE_PATH_OK:
      return 0;

    default: {
      PyErr_Format(PyExc_TypeError, "unexpected stuff happened");
      return -1;
    }
  }
}

static PyMappingMethods fastmanifest_mapping_methods = {
    (lenfunc)fastmanifest_size, /* mp_length */
    (binaryfunc)fastmanifest_getitem, /* mp_subscript */
    (objobjargproc)fastmanifest_setitem, /* mp_ass_subscript */
};

/* sequence methods (important or __contains__ builds an iterator) */

static int fastmanifest_contains(fastmanifest* self, PyObject* key) {
  if (!fastmanifest_is_valid_manifest_key(key)) {
    /* Our keys are always strings, so if the contains
     * check is for a non-string, just return false. */
    return 0;
  }
  char* path;
  ssize_t plen;
  int err = PyString_AsStringAndSize(key, &path, &plen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError, "Error decoding path");
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
    {"iterkeys",
     (PyCFunction)fastmanifest_getkeysiter,
     METH_NOARGS,
     "Iterate over file names in this fastmanifest."},
    {"iterentries",
     (PyCFunction)fastmanifest_getentriesiter,
     METH_NOARGS,
     "Iterate over (path, nodeid, flags) tuples in this fastmanifest."},
    {"copy",
     (PyCFunction)fastmanifest_copy,
     METH_NOARGS,
     "Make a copy of this fastmanifest."},
    {"filtercopy",
     (PyCFunction)fastmanifest_filtercopy,
     METH_O,
     "Make a copy of this manifest filtered by matchfn."},
    {"_save",
     (PyCFunction)fastmanifest_save,
     METH_VARARGS,
     "Save a fastmanifest to a file"},
    {"load",
     (PyCFunction)fastmanifest_load,
     METH_VARARGS | METH_CLASS,
     "Load a tree manifest from a file"},
    {"diff",
     (PyCFunction)fastmanifest_diff,
     METH_VARARGS | METH_KEYWORDS,
     "Compare this fastmanifest to another one."},
    {"text",
     (PyCFunction)fastmanifest_text,
     METH_NOARGS,
     "Encode this manifest to text."},
    {"bytes",
     (PyCFunction)fastmanifest_bytes,
     METH_NOARGS,
     "Returns an upper bound on the number of bytes required "
     "to represent this manifest."},
    {NULL},
};

static PyTypeObject fastmanifestType = {
    PyObject_HEAD_INIT(NULL) 0, /* ob_size */
    "parsers.fastmanifest", /* tp_name */
    sizeof(fastmanifest), /* tp_basicsize */
    0, /* tp_itemsize */
    (destructor)fastmanifest_dealloc, /* tp_dealloc */
    0, /* tp_print */
    0, /* tp_getattr */
    0, /* tp_setattr */
    0, /* tp_compare */
    0, /* tp_repr */
    0, /* tp_as_number */
    &fastmanifest_seq_meths, /* tp_as_sequence */
    &fastmanifest_mapping_methods, /* tp_as_mapping */
    0, /* tp_hash */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */
    Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_SEQUENCE_IN, /* tp_flags */
    "TODO(augie)", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    (getiterfunc)fastmanifest_getkeysiter, /* tp_iter */
    0, /* tp_iternext */
    fastmanifest_methods, /* tp_methods */
    0, /* tp_members */
    0, /* tp_getset */
    0, /* tp_base */
    0, /* tp_dict */
    0, /* tp_descr_get */
    0, /* tp_descr_set */
    0, /* tp_dictoffset */
    (initproc)fastmanifest_init, /* tp_init */
    0, /* tp_alloc */
};

/* iteration support */

static void fmiter_dealloc(PyObject* o) {
  fmIter* self = (fmIter*)o;
  destroy_iterator(self->iterator);
  PyObject_Del(self);
}

static PyObject* fmiter_iterkeysnext(PyObject* o) {
  fmIter* self = (fmIter*)o;
  iterator_result_t iterator_result = iterator_next(self->iterator);
  if (!iterator_result.valid) {
    return NULL;
  }
  return PyString_FromStringAndSize(
      iterator_result.path, iterator_result.path_sz);
}

static PyObject* fmiter_iterentriesnext(PyObject* o) {
  fmIter* self = (fmIter*)o;
  iterator_result_t iterator_result = iterator_next(self->iterator);
  if (!iterator_result.valid) {
    return NULL;
  }

  PyObject *ret = NULL, *path, *hash, *flags;
  path =
      PyString_FromStringAndSize(iterator_result.path, iterator_result.path_sz);
  hash = PyString_FromStringAndSize(
      (const char*)iterator_result.checksum, iterator_result.checksum_sz);
  flags = PyString_FromStringAndSize(
      (const char*)&iterator_result.flags, iterator_result.flags == 0 ? 0 : 1);
  if (!path || !hash || !flags) {
    goto done;
  }
  ret = PyTuple_Pack(3, path, hash, flags);
done:
  Py_XDECREF(path);
  Py_XDECREF(hash);
  Py_XDECREF(flags);
  return ret;
}

static PyTypeObject fastmanifestKeysIterator = {
    PyObject_HEAD_INIT(NULL) 0, /*ob_size */
    "parsers.fastmanifest.keysiterator", /*tp_name */
    sizeof(fmIter), /*tp_basicsize */
    0, /*tp_itemsize */
    fmiter_dealloc, /*tp_dealloc */
    0, /*tp_print */
    0, /*tp_getattr */
    0, /*tp_setattr */
    0, /*tp_compare */
    0, /*tp_repr */
    0, /*tp_as_number */
    0, /*tp_as_sequence */
    0, /*tp_as_mapping */
    0, /*tp_hash */
    0, /*tp_call */
    0, /*tp_str */
    0, /*tp_getattro */
    0, /*tp_setattro */
    0, /*tp_as_buffer */
    /* tp_flags: Py_TPFLAGS_HAVE_ITER tells python to
       use tp_iter and tp_iternext fields. */
    Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_ITER,
    "Keys iterator for a fastmanifest.", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    PyObject_SelfIter, /* tp_iter: __iter__() method */
    fmiter_iterkeysnext, /* tp_iternext: next() method */
};

static PyTypeObject fastmanifestEntriesIterator = {
    PyObject_HEAD_INIT(NULL) 0, /*ob_size */
    "parsers.fastmanifest.entriesiterator", /*tp_name */
    sizeof(fmIter), /*tp_basicsize */
    0, /*tp_itemsize */
    fmiter_dealloc, /*tp_dealloc */
    0, /*tp_print */
    0, /*tp_getattr */
    0, /*tp_setattr */
    0, /*tp_compare */
    0, /*tp_repr */
    0, /*tp_as_number */
    0, /*tp_as_sequence */
    0, /*tp_as_mapping */
    0, /*tp_hash */
    0, /*tp_call */
    0, /*tp_str */
    0, /*tp_getattro */
    0, /*tp_setattro */
    0, /*tp_as_buffer */
    /* tp_flags: Py_TPFLAGS_HAVE_ITER tells python to
       use tp_iter and tp_iternext fields. */
    Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_ITER,
    "Iterator for 3-tuples in a fastmanifest.", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    PyObject_SelfIter, /* tp_iter: __iter__() method */
    fmiter_iterentriesnext, /* tp_iternext: next() method */
};

static PyMethodDef methods[] = {{NULL, NULL, 0, NULL}};

PyMODINIT_FUNC initcfastmanifest(void) {
  PyObject* m;

  fastmanifestType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&fastmanifestType) < 0)
    return;

  m = Py_InitModule3("cfastmanifest", methods, "Wrapper around fast_manifest");

  Py_INCREF(&fastmanifestType);
  PyModule_AddObject(m, "fastmanifest", (PyObject*)&fastmanifestType);
}
