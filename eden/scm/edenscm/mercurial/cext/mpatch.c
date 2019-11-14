/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 mpatch.c - efficient binary patching for Mercurial

 This implements a patch algorithm that's O(m + nlog n) where m is the
 size of the output and n is the number of patches.

 Given a list of binary patches, it unpacks each into a hunk list,
 then combines the hunk lists with a treewise recursion to form a
 single hunk list. This hunk list is then applied to the original
 text.

 The text (or binary) fragments are copied directly from their source
 Python objects into a preallocated output string to avoid the
 allocation of intermediate Python objects. Working memory is about 2x
 the total number of hunks.

 Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>

 This software may be used and distributed according to the terms
 of the GNU General Public License, incorporated herein by reference.
*/

#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <stdlib.h>
#include <string.h>

#include "edenscm/mercurial/bitmanipulation.h"
#include "edenscm/mercurial/compat.h"
#include "edenscm/mercurial/mpatch.h"
#include "util.h"

static char mpatch_doc[] = "Efficient binary patching.";
static PyObject* mpatch_Error;

static void setpyerr(int r) {
  switch (r) {
    case MPATCH_ERR_NO_MEM:
      PyErr_NoMemory();
      break;
    case MPATCH_ERR_CANNOT_BE_DECODED:
      PyErr_SetString(mpatch_Error, "patch cannot be decoded");
      break;
    case MPATCH_ERR_INVALID_PATCH:
      PyErr_SetString(mpatch_Error, "invalid patch");
      break;
  }
}

struct mpatch_flist* cpygetitem(void* bins, ssize_t pos) {
  const char* buffer;
  struct mpatch_flist* res;
  ssize_t blen;
  int r;

  PyObject* tmp = PyList_GetItem((PyObject*)bins, pos);
  if (!tmp)
    return NULL;
  if (PyObject_AsCharBuffer(tmp, &buffer, (Py_ssize_t*)&blen))
    return NULL;
  if ((r = mpatch_decode(buffer, blen, &res)) < 0) {
    if (!PyErr_Occurred())
      setpyerr(r);
    return NULL;
  }
  return res;
}

static PyObject* patches(PyObject* self, PyObject* args) {
  PyObject *text, *bins, *result;
  struct mpatch_flist* patch;
  const char* in;
  int r = 0;
  char* out;
  Py_ssize_t len, outlen, inlen;

  if (!PyArg_ParseTuple(args, "OO:mpatch", &text, &bins))
    return NULL;

  len = PyList_Size(bins);
  if (!len) {
    /* nothing to do */
    Py_INCREF(text);
    return text;
  }

  if (PyObject_AsCharBuffer(text, &in, &inlen))
    return NULL;

  patch = mpatch_fold(bins, cpygetitem, 0, len);
  if (!patch) { /* error already set or memory error */
    if (!PyErr_Occurred())
      PyErr_NoMemory();
    return NULL;
  }

  outlen = mpatch_calcsize(inlen, patch);
  if (outlen < 0) {
    r = (int)outlen;
    result = NULL;
    goto cleanup;
  }
  result = PyBytes_FromStringAndSize(NULL, outlen);
  if (!result) {
    result = NULL;
    goto cleanup;
  }
  out = PyBytes_AsString(result);
  if ((r = mpatch_apply(out, in, inlen, patch)) < 0) {
    Py_DECREF(result);
    result = NULL;
  }
cleanup:
  mpatch_lfree(patch);
  if (!result && !PyErr_Occurred())
    setpyerr(r);
  return result;
}

/* calculate size of a patched file directly */
static PyObject* patchedsize(PyObject* self, PyObject* args) {
  long orig, start, end, len, outlen = 0, last = 0, pos = 0;
  Py_ssize_t patchlen;
  char* bin;

  if (!PyArg_ParseTuple(args, "ls#", &orig, &bin, &patchlen))
    return NULL;

  while (pos >= 0 && pos < patchlen) {
    start = getbe32(bin + pos);
    end = getbe32(bin + pos + 4);
    len = getbe32(bin + pos + 8);
    if (start > end)
      break; /* sanity check */
    pos += 12 + len;
    outlen += start - last;
    last = end;
    outlen += len;
  }

  if (pos != patchlen) {
    if (!PyErr_Occurred())
      PyErr_SetString(mpatch_Error, "patch cannot be decoded");
    return NULL;
  }

  outlen += orig - last;
  return Py_BuildValue("l", outlen);
}

static PyMethodDef methods[] = {
    {"patches", patches, METH_VARARGS, "apply a series of patches\n"},
    {"patchedsize", patchedsize, METH_VARARGS, "calculed patched size\n"},
    {NULL, NULL}};

static const int version = 1;

#ifdef IS_PY3K
static struct PyModuleDef mpatch_module = {PyModuleDef_HEAD_INIT,
                                           "mpatch",
                                           mpatch_doc,
                                           -1,
                                           methods};

PyMODINIT_FUNC PyInit_mpatch(void) {
  PyObject* m;

  m = PyModule_Create(&mpatch_module);
  if (m == NULL)
    return NULL;

  mpatch_Error =
      PyErr_NewException("mercurial.cext.mpatch.mpatchError", NULL, NULL);
  Py_INCREF(mpatch_Error);
  PyModule_AddObject(m, "mpatchError", mpatch_Error);
  PyModule_AddIntConstant(m, "version", version);

  return m;
}
#else
PyMODINIT_FUNC initmpatch(void) {
  PyObject* m;
  m = Py_InitModule3("mpatch", methods, mpatch_doc);
  mpatch_Error =
      PyErr_NewException("mercurial.cext.mpatch.mpatchError", NULL, NULL);
  PyModule_AddIntConstant(m, "version", version);
}
#endif
