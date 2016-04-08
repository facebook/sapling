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


/* TODO @ttung replace this with your structs and fix
   reference */
#define MANIFEST_OOM -1
#define MANIFEST_NOT_SORTED -2
#define MANIFEST_MALFORMED -3
#define DEFAULT_NUM_CHILDREN 12
#define NULLHASH "00000000000000000000"

typedef struct tmnode {
  char *start;
  Py_ssize_t len;
  char hash_suffix;
  struct tmnode *children;
  int numchildren;
  int maxchildren;
} tmnode;

enum tmquerystatus {
  OOM,
  NOT_FOUND,
  FOUND_DIR,
  FOUND_FILE,
};

typedef struct {
  PyObject_HEAD;
  tmnode *root;
} fastmanifest;

typedef struct tmquery {
  enum tmquerystatus status;
  tmnode *node;
} tmquery;


static PyTypeObject fastmanifestType;

/* ========================== */
/* Fastmanifest: C Interface */
/* ========================== */

/* Deallocate all the nodes in the tree */
static void ifastmanifest_dealloc(fastmanifest *self)
{
  /* TODO integration with @ttung */
}

static fastmanifest *ifastmanifest_copy(fastmanifest *copy, fastmanifest *self)
{
  /* TODO integration with @ttung */
}

static void ifastmanifest_save(fastmanifest *copy, char *filename, int len)
{
  /* TODO integration with @ttung */
}

static void ifastmanifest_load(fastmanifest *copy, char *filename, int len)
{
  /* TODO integration with @ttung */
}

static tmquery ifastmanifest_getitem(fastmanifest *self, char *path, int plen)
{
  /* TODO Integration with @ttung */
}

static int ifastmanifest_insert(fastmanifest *self,
                                char *path, ssize_t plen,
                                char *hash, ssize_t hlen,
                                char *flags, ssize_t flen, bool ishexhash)
{
  /* TODO Integration with @ttung */
}


static int ifastmanifest_insert_lines(fastmanifest *self, char *data,
                    Py_ssize_t len)
{
  while (len > 0) {
    char *next = memchr(data, '\n', len);
    if (!next) {
      return MANIFEST_MALFORMED;
    }

    next++; /* advance past newline */
    int llen = next - data;
    int plen = strlen(data);
    char *hash = data + plen + 1;
    char *flags = data + plen + 42;
    int hlen = 40;
    int flen = next - flags;
    ifastmanifest_insert(self, data, plen, hash, hlen, flags, flen, true);
    len = len - llen;
    data = next;
  }
  return 0;
}

static int ifastmanifest_init(fastmanifest *self, char *data, ssize_t len)
{
  /* TODO Integration with @ttung */
}

static ssize_t ifastmanifest_size(fastmanifest *self)
{
  /* TODO Integration with @ttung */
}

static tmquery ifastmanifest_delitem(fastmanifest *tm, char *path, int plen)
{
  /* TODO Integration with @ttung */
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

/* get the node value of tmnode */
static PyObject *tm_nodeof(tmnode *l) {
  char *s = l->start;
  ssize_t llen = strlen(l->start);
  PyObject *hash = unhexlify(s + llen + 1, 40);
  if (!hash) {
    return NULL;
  }
  if (l->hash_suffix != '\0') {
    char newhash[21];
    memcpy(newhash, PyString_AsString(hash), 20);
    Py_DECREF(hash);
    newhash[20] = l->hash_suffix;
    hash = PyString_FromStringAndSize(newhash, 21);
  }
  return hash;
}

static PyObject *fastmanifest_formatfile(tmnode *l) {
  if (l == NULL) {
    return NULL;
  }
  char *s = l->start;
  size_t plen = strlen(l->start);
  PyObject *hash = tm_nodeof(l);

  /* 40 for hash, 1 for null byte, 1 for newline */
  size_t hplen = plen + 42;
  Py_ssize_t flen = l->len - hplen;
  PyObject *flags;
  PyObject *tup;

  if (!hash)
    return NULL;
  flags = PyString_FromStringAndSize(s + hplen - 1, flen);
  if (!flags) {
    Py_DECREF(hash);
    return NULL;
  }
  tup = PyTuple_Pack(2, hash, flags);
  Py_DECREF(flags);
  Py_DECREF(hash);
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
  err = ifastmanifest_init(self, data, len);
  switch(err) {
  case MANIFEST_OOM:
    PyErr_NoMemory();
    return -1;
  case MANIFEST_MALFORMED:
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
    return -1;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1)
    return -1;
  /* TODO @ttung error handling */
  ifastmanifest_save(self, data, len);
	return NULL;
}

static PyObject *fastmanifest_load(fastmanifest *self, PyObject *args) {
  PyObject *pydata = NULL;
  char *data;
  ssize_t len;
  if (!PyArg_ParseTuple(args, "S", &pydata)) {
    return -1;
  }
  int err = PyString_AsStringAndSize(pydata, &data, &len);
  if (err == -1)
    return -1;
  /* TODO @ttung error handling */
  ifastmanifest_load(self, data, len);
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

  tmquery query = ifastmanifest_getitem(self, ckey, clen);
  switch (query.status) {
  case OOM:
    PyErr_NoMemory();
    break;

  case FOUND_DIR:
    PyErr_Format(PyExc_ValueError,
           "Found dir matching path, expected file");
    break;

  case NOT_FOUND:
    PyErr_Format(PyExc_ValueError,
           "File not found");
    break;
  default:
    break;
  }

  if (query.status != FOUND_FILE) {
    PyErr_Format(PyExc_KeyError, "File not found");
    return NULL;
  }

  PyObject *ret = fastmanifest_formatfile(query.node);
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
  tmquery response;
  /* Decode path */
  if (!fastmanifest_is_valid_manifest_key(key)) {
    return -1;
  }
  err = PyString_AsStringAndSize(key, &path, &plen);
  if (err == -1) {
    PyErr_Format(PyExc_TypeError,
           "Error decoding path");
    return -1;
  }

  if (!value) {
    response = ifastmanifest_delitem(self, path, plen);

    switch(response.status) {

    case OOM:
      PyErr_NoMemory();
      return -1;

    case FOUND_FILE:
      return 0;

    case FOUND_DIR:
      PyErr_Format(PyExc_KeyError,
             "Cannot delete manifest dir");
      return -1;

    case NOT_FOUND:
      PyErr_Format(PyExc_KeyError,
             "Not found");
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

  err = ifastmanifest_insert(self, path, plen, hash, hlen, flags, flen, false);
  if (err == MANIFEST_OOM) {
    PyErr_NoMemory();
    return -1;
  }

  return 0;
}

static PyMappingMethods fastmanifest_mapping_methods = {
  (lenfunc)fastmanifest_size,          /* mp_length */
  (binaryfunc)fastmanifest_getitem,    /* mp_subscript */
  (objobjargproc)fastmanifest_setitem, /* mp_ass_subscript */
};

static PySequenceMethods fastmanifest_seq_meths = {
  (lenfunc)fastmanifest_size, /* sq_length */
  0, /* sq_concat */
  0, /* sq_repeat */
  0, /* sq_item */
  0, /* sq_slice */
  0, /* sq_ass_item */
  0, /* sq_ass_slice */
  0, /* sq_contains */
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
