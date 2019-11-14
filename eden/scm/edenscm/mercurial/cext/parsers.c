/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 parsers.c - efficient content parsing

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <ctype.h>
#include <stddef.h>
#include <string.h>

#include "edenscm/mercurial/bitmanipulation.h"
#include "edenscm/mercurial/cext/charencode.h"
#include "edenscm/mercurial/cext/util.h"

#ifdef IS_PY3K
/* The mapping of Python types is meant to be temporary to get Python
 * 3 to compile. We should remove this once Python 3 support is fully
 * supported and proper types are used in the extensions themselves. */
#define PyInt_Check PyLong_Check
#define PyInt_FromLong PyLong_FromLong
#define PyInt_FromSsize_t PyLong_FromSsize_t
#define PyInt_AsLong PyLong_AsLong
#endif

static const char* const versionerrortext = "Python minor version mismatch";

static PyObject* dict_new_presized(PyObject* self, PyObject* args) {
  Py_ssize_t expected_size;

  if (!PyArg_ParseTuple(args, "n:make_presized_dict", &expected_size))
    return NULL;

  return _dict_new_presized(expected_size);
}

/*
 * This code assumes that a manifest is stitched together with newline
 * ('\n') characters.
 */
static PyObject* parse_manifest(PyObject* self, PyObject* args) {
  PyObject *mfdict, *fdict;
  char *str, *start, *end;
  int len;

  if (!PyArg_ParseTuple(
          args,
          "O!O!s#:parse_manifest",
          &PyDict_Type,
          &mfdict,
          &PyDict_Type,
          &fdict,
          &str,
          &len))
    goto quit;

  start = str;
  end = str + len;
  while (start < end) {
    PyObject *file = NULL, *node = NULL;
    PyObject* flags = NULL;
    char *zero = NULL, *newline = NULL;
    ptrdiff_t nlen;

    zero = memchr(start, '\0', end - start);
    if (!zero) {
      PyErr_SetString(PyExc_ValueError, "manifest entry has no separator");
      goto quit;
    }

    newline = memchr(zero + 1, '\n', end - (zero + 1));
    if (!newline) {
      PyErr_SetString(PyExc_ValueError, "manifest contains trailing garbage");
      goto quit;
    }

    file = PyBytes_FromStringAndSize(start, zero - start);

    if (!file)
      goto bail;

    nlen = newline - zero - 1;

    node = unhexlify(zero + 1, nlen > 40 ? 40 : (Py_ssize_t)nlen);
    if (!node)
      goto bail;

    if (nlen > 40) {
      flags = PyBytes_FromStringAndSize(zero + 41, nlen - 40);
      if (!flags)
        goto bail;

      if (PyDict_SetItem(fdict, file, flags) == -1)
        goto bail;
    }

    if (PyDict_SetItem(mfdict, file, node) == -1)
      goto bail;

    start = newline + 1;

    Py_XDECREF(flags);
    Py_XDECREF(node);
    Py_XDECREF(file);
    continue;
  bail:
    Py_XDECREF(flags);
    Py_XDECREF(node);
    Py_XDECREF(file);
    goto quit;
  }

  Py_INCREF(Py_None);
  return Py_None;
quit:
  return NULL;
}

static inline dirstateTupleObject*
make_dirstate_tuple(char state, int mode, int size, int mtime) {
  dirstateTupleObject* t =
      PyObject_New(dirstateTupleObject, &dirstateTupleType);
  if (!t)
    return NULL;
  t->state = state;
  t->mode = mode;
  t->size = size;
  t->mtime = mtime;
  return t;
}

static PyObject*
dirstate_tuple_new(PyTypeObject* subtype, PyObject* args, PyObject* kwds) {
  /* We do all the initialization here and not a tp_init function because
   * dirstate_tuple is immutable. */
  dirstateTupleObject* t;
  char state;
  int size, mode, mtime;
  if (!PyArg_ParseTuple(args, "ciii", &state, &mode, &size, &mtime))
    return NULL;

  t = (dirstateTupleObject*)subtype->tp_alloc(subtype, 1);
  if (!t)
    return NULL;
  t->state = state;
  t->mode = mode;
  t->size = size;
  t->mtime = mtime;

  return (PyObject*)t;
}

static void dirstate_tuple_dealloc(PyObject* o) {
  PyObject_Del(o);
}

static Py_ssize_t dirstate_tuple_length(PyObject* o) {
  return 4;
}

static PyObject* dirstate_tuple_item(PyObject* o, Py_ssize_t i) {
  dirstateTupleObject* t = (dirstateTupleObject*)o;
  switch (i) {
    case 0:
      return PyBytes_FromStringAndSize(&t->state, 1);
    case 1:
      return PyInt_FromLong(t->mode);
    case 2:
      return PyInt_FromLong(t->size);
    case 3:
      return PyInt_FromLong(t->mtime);
    default:
      PyErr_SetString(PyExc_IndexError, "index out of range");
      return NULL;
  }
}

static PySequenceMethods dirstate_tuple_sq = {
    dirstate_tuple_length, /* sq_length */
    0, /* sq_concat */
    0, /* sq_repeat */
    dirstate_tuple_item, /* sq_item */
    0, /* sq_ass_item */
    0, /* sq_contains */
    0, /* sq_inplace_concat */
    0 /* sq_inplace_repeat */
};

PyTypeObject dirstateTupleType = {
    PyVarObject_HEAD_INIT(NULL, 0) /* header */
    "dirstate_tuple", /* tp_name */
    sizeof(dirstateTupleObject), /* tp_basicsize */
    0, /* tp_itemsize */
    (destructor)dirstate_tuple_dealloc, /* tp_dealloc */
    0, /* tp_print */
    0, /* tp_getattr */
    0, /* tp_setattr */
    0, /* tp_compare */
    0, /* tp_repr */
    0, /* tp_as_number */
    &dirstate_tuple_sq, /* tp_as_sequence */
    0, /* tp_as_mapping */
    0, /* tp_hash  */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */
    Py_TPFLAGS_DEFAULT, /* tp_flags */
    "dirstate tuple", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    0, /* tp_iter */
    0, /* tp_iternext */
    0, /* tp_methods */
    0, /* tp_members */
    0, /* tp_getset */
    0, /* tp_base */
    0, /* tp_dict */
    0, /* tp_descr_get */
    0, /* tp_descr_set */
    0, /* tp_dictoffset */
    0, /* tp_init */
    0, /* tp_alloc */
    dirstate_tuple_new, /* tp_new */
};

static PyObject* parse_dirstate(PyObject* self, PyObject* args) {
  PyObject *dmap, *cmap, *parents = NULL, *ret = NULL;
  PyObject *fname = NULL, *cname = NULL, *entry = NULL;
  char state, *cur, *str, *cpos;
  int mode, size, mtime;
  unsigned int flen, len, pos = 40;
  int readlen;

  if (!PyArg_ParseTuple(
          args,
          "O!O!s#:parse_dirstate",
          &PyDict_Type,
          &dmap,
          &PyDict_Type,
          &cmap,
          &str,
          &readlen))
    goto quit;

  len = readlen;

  /* read parents */
  if (len < 40) {
    PyErr_SetString(PyExc_ValueError, "too little data for parents");
    goto quit;
  }

  parents = Py_BuildValue("s#s#", str, 20, str + 20, 20);
  if (!parents)
    goto quit;

  /* read filenames */
  while (pos >= 40 && pos < len) {
    if (pos + 17 > len) {
      PyErr_SetString(PyExc_ValueError, "overflow in dirstate");
      goto quit;
    }
    cur = str + pos;
    /* unpack header */
    state = *cur;
    mode = getbe32(cur + 1);
    size = getbe32(cur + 5);
    mtime = getbe32(cur + 9);
    flen = getbe32(cur + 13);
    pos += 17;
    cur += 17;
    if (flen > len - pos) {
      PyErr_SetString(PyExc_ValueError, "overflow in dirstate");
      goto quit;
    }

    entry = (PyObject*)make_dirstate_tuple(state, mode, size, mtime);
    cpos = memchr(cur, 0, flen);
    if (cpos) {
      fname = PyBytes_FromStringAndSize(cur, cpos - cur);
      cname = PyBytes_FromStringAndSize(cpos + 1, flen - (cpos - cur) - 1);
      if (!fname || !cname || PyDict_SetItem(cmap, fname, cname) == -1 ||
          PyDict_SetItem(dmap, fname, entry) == -1)
        goto quit;
      Py_DECREF(cname);
    } else {
      fname = PyBytes_FromStringAndSize(cur, flen);
      if (!fname || PyDict_SetItem(dmap, fname, entry) == -1)
        goto quit;
    }
    Py_DECREF(fname);
    Py_DECREF(entry);
    fname = cname = entry = NULL;
    pos += flen;
  }

  ret = parents;
  Py_INCREF(ret);
quit:
  Py_XDECREF(fname);
  Py_XDECREF(cname);
  Py_XDECREF(entry);
  Py_XDECREF(parents);
  return ret;
}

/*
 * Build a set of non-normal and other parent entries from the dirstate dmap
 */
static PyObject* nonnormalotherparententries(PyObject* self, PyObject* args) {
  PyObject *dmap, *fname, *v;
  PyObject *nonnset = NULL, *otherpset = NULL, *result = NULL;
  Py_ssize_t pos;

  if (!PyArg_ParseTuple(args, "O!:nonnormalentries", &PyDict_Type, &dmap))
    goto bail;

  nonnset = PySet_New(NULL);
  if (nonnset == NULL)
    goto bail;

  otherpset = PySet_New(NULL);
  if (otherpset == NULL)
    goto bail;

  pos = 0;
  while (PyDict_Next(dmap, &pos, &fname, &v)) {
    dirstateTupleObject* t;
    if (!dirstate_tuple_check(v)) {
      PyErr_SetString(PyExc_TypeError, "expected a dirstate tuple");
      goto bail;
    }
    t = (dirstateTupleObject*)v;

    if (t->state == 'n' && t->size == -2) {
      if (PySet_Add(otherpset, fname) == -1) {
        goto bail;
      }
    }

    if (t->state == 'n' && t->mtime != -1)
      continue;
    if (PySet_Add(nonnset, fname) == -1)
      goto bail;
  }

  result = Py_BuildValue("(OO)", nonnset, otherpset);
  if (result == NULL)
    goto bail;
  Py_DECREF(nonnset);
  Py_DECREF(otherpset);
  return result;
bail:
  Py_XDECREF(nonnset);
  Py_XDECREF(otherpset);
  Py_XDECREF(result);
  return NULL;
}

/*
 * Efficiently pack a dirstate object into its on-disk format.
 */
static PyObject* pack_dirstate(PyObject* self, PyObject* args) {
  PyObject* packobj = NULL;
  PyObject *map, *copymap, *pl, *mtime_unset = NULL;
  Py_ssize_t nbytes, pos, l;
  PyObject *k, *v = NULL, *pn;
  char *p, *s;
  int now;

  if (!PyArg_ParseTuple(
          args,
          "O!O!Oi:pack_dirstate",
          &PyDict_Type,
          &map,
          &PyDict_Type,
          &copymap,
          &pl,
          &now))
    return NULL;

  if (!PySequence_Check(pl) || PySequence_Size(pl) != 2) {
    PyErr_SetString(PyExc_TypeError, "expected 2-element sequence");
    return NULL;
  }

  /* Figure out how much we need to allocate. */
  for (nbytes = 40, pos = 0; PyDict_Next(map, &pos, &k, &v);) {
    PyObject* c;
    if (!PyBytes_Check(k)) {
      PyErr_SetString(PyExc_TypeError, "expected string key");
      goto bail;
    }
    nbytes += PyBytes_GET_SIZE(k) + 17;
    c = PyDict_GetItem(copymap, k);
    if (c) {
      if (!PyBytes_Check(c)) {
        PyErr_SetString(PyExc_TypeError, "expected string key");
        goto bail;
      }
      nbytes += PyBytes_GET_SIZE(c) + 1;
    }
  }

  packobj = PyBytes_FromStringAndSize(NULL, nbytes);
  if (packobj == NULL)
    goto bail;

  p = PyBytes_AS_STRING(packobj);

  pn = PySequence_ITEM(pl, 0);
  if (PyBytes_AsStringAndSize(pn, &s, &l) == -1 || l != 20) {
    PyErr_SetString(PyExc_TypeError, "expected a 20-byte hash");
    goto bail;
  }
  memcpy(p, s, l);
  p += 20;
  pn = PySequence_ITEM(pl, 1);
  if (PyBytes_AsStringAndSize(pn, &s, &l) == -1 || l != 20) {
    PyErr_SetString(PyExc_TypeError, "expected a 20-byte hash");
    goto bail;
  }
  memcpy(p, s, l);
  p += 20;

  for (pos = 0; PyDict_Next(map, &pos, &k, &v);) {
    dirstateTupleObject* tuple;
    char state;
    int mode, size, mtime;
    Py_ssize_t len, l1;
    PyObject* o;
    char* t;

    if (!dirstate_tuple_check(v)) {
      PyErr_SetString(PyExc_TypeError, "expected a dirstate tuple");
      goto bail;
    }
    tuple = (dirstateTupleObject*)v;

    state = tuple->state;
    mode = tuple->mode;
    size = tuple->size;
    mtime = tuple->mtime;
    if (state == 'n' && mtime == now) {
      /* See pure/parsers.py:pack_dirstate for why we do
       * this. */
      mtime = -1;
      mtime_unset = (PyObject*)make_dirstate_tuple(state, mode, size, mtime);
      if (!mtime_unset)
        goto bail;
      if (PyDict_SetItem(map, k, mtime_unset) == -1)
        goto bail;
      Py_DECREF(mtime_unset);
      mtime_unset = NULL;
    }
    *p++ = state;
    putbe32((uint32_t)mode, p);
    putbe32((uint32_t)size, p + 4);
    putbe32((uint32_t)mtime, p + 8);
    t = p + 12;
    p += 16;
    len = PyBytes_GET_SIZE(k);
    memcpy(p, PyBytes_AS_STRING(k), len);
    p += len;
    o = PyDict_GetItem(copymap, k);
    if (o) {
      *p++ = '\0';
      l1 = PyBytes_GET_SIZE(o);
      memcpy(p, PyBytes_AS_STRING(o), l1);
      p += l1;
      len += l1 + 1;
    }
    putbe32((uint32_t)len, t);
  }

  pos = p - PyBytes_AS_STRING(packobj);
  if (pos != nbytes) {
    PyErr_Format(
        PyExc_SystemError,
        "bad dirstate size: %ld != %ld",
        (long)pos,
        (long)nbytes);
    goto bail;
  }

  return packobj;
bail:
  Py_XDECREF(mtime_unset);
  Py_XDECREF(packobj);
  Py_XDECREF(v);
  return NULL;
}

#define BUMPED_FIX 1
#define USING_SHA_256 2
#define FM1_HEADER_SIZE (4 + 8 + 2 + 2 + 1 + 1 + 1)

static PyObject*
readshas(const char* source, unsigned char num, Py_ssize_t hashwidth) {
  int i;
  PyObject* list = PyTuple_New(num);
  if (list == NULL) {
    return NULL;
  }
  for (i = 0; i < num; i++) {
    PyObject* hash = PyBytes_FromStringAndSize(source, hashwidth);
    if (hash == NULL) {
      Py_DECREF(list);
      return NULL;
    }
    PyTuple_SET_ITEM(list, i, hash);
    source += hashwidth;
  }
  return list;
}

static PyObject*
fm1readmarker(const char* databegin, const char* dataend, uint32_t* msize) {
  const char* data = databegin;
  const char* meta;

  double mtime;
  int16_t tz;
  uint16_t flags;
  unsigned char nsuccs, nparents, nmetadata;
  Py_ssize_t hashwidth = 20;

  PyObject *prec = NULL, *parents = NULL, *succs = NULL;
  PyObject *metadata = NULL, *ret = NULL;
  int i;

  if (data + FM1_HEADER_SIZE > dataend) {
    goto overflow;
  }

  *msize = getbe32(data);
  data += 4;
  mtime = getbefloat64(data);
  data += 8;
  tz = getbeint16(data);
  data += 2;
  flags = getbeuint16(data);
  data += 2;

  if (flags & USING_SHA_256) {
    hashwidth = 32;
  }

  nsuccs = (unsigned char)(*data++);
  nparents = (unsigned char)(*data++);
  nmetadata = (unsigned char)(*data++);

  if (databegin + *msize > dataend) {
    goto overflow;
  }
  dataend = databegin + *msize; /* narrow down to marker size */

  if (data + hashwidth > dataend) {
    goto overflow;
  }
  prec = PyBytes_FromStringAndSize(data, hashwidth);
  data += hashwidth;
  if (prec == NULL) {
    goto bail;
  }

  if (data + nsuccs * hashwidth > dataend) {
    goto overflow;
  }
  succs = readshas(data, nsuccs, hashwidth);
  if (succs == NULL) {
    goto bail;
  }
  data += nsuccs * hashwidth;

  if (nparents == 1 || nparents == 2) {
    if (data + nparents * hashwidth > dataend) {
      goto overflow;
    }
    parents = readshas(data, nparents, hashwidth);
    if (parents == NULL) {
      goto bail;
    }
    data += nparents * hashwidth;
  } else {
    parents = Py_None;
    Py_INCREF(parents);
  }

  if (data + 2 * nmetadata > dataend) {
    goto overflow;
  }
  meta = data + (2 * nmetadata);
  metadata = PyTuple_New(nmetadata);
  if (metadata == NULL) {
    goto bail;
  }
  for (i = 0; i < nmetadata; i++) {
    PyObject *tmp, *left = NULL, *right = NULL;
    Py_ssize_t leftsize = (unsigned char)(*data++);
    Py_ssize_t rightsize = (unsigned char)(*data++);
    if (meta + leftsize + rightsize > dataend) {
      goto overflow;
    }
    left = PyBytes_FromStringAndSize(meta, leftsize);
    meta += leftsize;
    right = PyBytes_FromStringAndSize(meta, rightsize);
    meta += rightsize;
    tmp = PyTuple_New(2);
    if (!left || !right || !tmp) {
      Py_XDECREF(left);
      Py_XDECREF(right);
      Py_XDECREF(tmp);
      goto bail;
    }
    PyTuple_SET_ITEM(tmp, 0, left);
    PyTuple_SET_ITEM(tmp, 1, right);
    PyTuple_SET_ITEM(metadata, i, tmp);
  }
  ret = Py_BuildValue(
      "(OOHO(di)O)",
      prec,
      succs,
      flags,
      metadata,
      mtime,
      (int)tz * 60,
      parents);
  goto bail; /* return successfully */

overflow:
  PyErr_SetString(PyExc_ValueError, "overflow in obsstore");
bail:
  Py_XDECREF(prec);
  Py_XDECREF(succs);
  Py_XDECREF(metadata);
  Py_XDECREF(parents);
  return ret;
}

static PyObject* fm1readmarkers(PyObject* self, PyObject* args) {
  const char *data, *dataend;
  int datalen;
  Py_ssize_t offset, stop;
  PyObject* markers = NULL;

  if (!PyArg_ParseTuple(args, "s#nn", &data, &datalen, &offset, &stop)) {
    return NULL;
  }
  dataend = data + datalen;
  data += offset;
  markers = PyList_New(0);
  if (!markers) {
    return NULL;
  }
  while (offset < stop) {
    uint32_t msize;
    int error;
    PyObject* record = fm1readmarker(data, dataend, &msize);
    if (!record) {
      goto bail;
    }
    error = PyList_Append(markers, record);
    Py_DECREF(record);
    if (error) {
      goto bail;
    }
    data += msize;
    offset += msize;
  }
  return markers;
bail:
  Py_DECREF(markers);
  return NULL;
}

static char parsers_doc[] = "Efficient content parsing.";

PyObject* encodedir(PyObject* self, PyObject* args);
PyObject* pathencode(PyObject* self, PyObject* args);
PyObject* lowerencode(PyObject* self, PyObject* args);
PyObject* parse_index2(PyObject* self, PyObject* args);

static PyMethodDef methods[] = {
    {"pack_dirstate", pack_dirstate, METH_VARARGS, "pack a dirstate\n"},
    {"nonnormalotherparententries",
     nonnormalotherparententries,
     METH_VARARGS,
     "create a set containing non-normal and other parent entries of given "
     "dirstate\n"},
    {"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
    {"parse_dirstate", parse_dirstate, METH_VARARGS, "parse a dirstate\n"},
    {"parse_index2", parse_index2, METH_VARARGS, "parse a revlog index\n"},
    {"isasciistr", isasciistr, METH_VARARGS, "check if an ASCII string\n"},
    {"asciilower", asciilower, METH_VARARGS, "lowercase an ASCII string\n"},
    {"asciiupper", asciiupper, METH_VARARGS, "uppercase an ASCII string\n"},
    {"dict_new_presized",
     dict_new_presized,
     METH_VARARGS,
     "construct a dict with an expected size\n"},
    {"make_file_foldmap",
     make_file_foldmap,
     METH_VARARGS,
     "make file foldmap\n"},
    {"jsonescapeu8fast",
     jsonescapeu8fast,
     METH_VARARGS,
     "escape a UTF-8 byte string to JSON (fast path)\n"},
    {"encodedir", encodedir, METH_VARARGS, "encodedir a path\n"},
    {"pathencode", pathencode, METH_VARARGS, "fncache-encode a path\n"},
    {"lowerencode", lowerencode, METH_VARARGS, "lower-encode a path\n"},
    {"fm1readmarkers",
     fm1readmarkers,
     METH_VARARGS,
     "parse v1 obsolete markers\n"},
    {NULL, NULL}};

void dirs_module_init(PyObject* mod);
void manifest_module_init(PyObject* mod);
void revlog_module_init(PyObject* mod);

static const int version = 5;

static void module_init(PyObject* mod) {
  PyModule_AddIntConstant(mod, "version", version);

  /* This module constant has two purposes.  First, it lets us unit test
   * the ImportError raised without hard-coding any error text.  This
   * means we can change the text in the future without breaking tests,
   * even across changesets without a recompile.  Second, its presence
   * can be used to determine whether the version-checking logic is
   * present, which also helps in testing across changesets without a
   * recompile.  Note that this means the pure-Python version of parsers
   * should not have this module constant. */
  PyModule_AddStringConstant(mod, "versionerrortext", versionerrortext);

  dirs_module_init(mod);
  manifest_module_init(mod);
  revlog_module_init(mod);

  if (PyType_Ready(&dirstateTupleType) < 0)
    return;
  Py_INCREF(&dirstateTupleType);
  PyModule_AddObject(mod, "dirstatetuple", (PyObject*)&dirstateTupleType);
}

static int check_python_version(void) {
  PyObject *sys = PyImport_ImportModule("sys"), *ver;
  long hexversion;
  if (!sys)
    return -1;
  ver = PyObject_GetAttrString(sys, "hexversion");
  Py_DECREF(sys);
  if (!ver)
    return -1;
  hexversion = PyInt_AsLong(ver);
  Py_DECREF(ver);
  /* sys.hexversion is a 32-bit number by default, so the -1 case
   * should only occur in unusual circumstances (e.g. if sys.hexversion
   * is manually set to an invalid value). */
  if ((hexversion == -1) || (hexversion >> 16 != PY_VERSION_HEX >> 16)) {
    PyErr_Format(
        PyExc_ImportError,
        "%s: The Mercurial extension "
        "modules were compiled with Python " PY_VERSION
        ", but "
        "Mercurial is currently using Python with "
        "sys.hexversion=%ld: "
        "Python %s\n at: %s",
        versionerrortext,
        hexversion,
        Py_GetVersion(),
        Py_GetProgramFullPath());
    return -1;
  }
  return 0;
}

#ifdef IS_PY3K
static struct PyModuleDef parsers_module = {PyModuleDef_HEAD_INIT,
                                            "parsers",
                                            parsers_doc,
                                            -1,
                                            methods};

PyMODINIT_FUNC PyInit_parsers(void) {
  PyObject* mod;

  if (check_python_version() == -1)
    return NULL;
  mod = PyModule_Create(&parsers_module);
  module_init(mod);
  return mod;
}
#else
PyMODINIT_FUNC initparsers(void) {
  PyObject* mod;

  if (check_python_version() == -1)
    return;
  mod = Py_InitModule3("parsers", methods, parsers_doc);
  module_init(mod);
}
#endif
