/*
 xdiff.c: simple Python wrapper for xdiff library

 Copyright (c) 2018 Facebook, Inc.

 This software may be used and distributed according to the terms of the
 GNU General Public License version 2 or any later version.
*/

#include "lib/third-party/xdiff/xdiff.h"
#include "Python.h"

#if PY_MAJOR_VERSION >= 3
#define IS_PY3K
#endif

static int
hunk_consumer(int64_t a1, int64_t a2, int64_t b1, int64_t b2, void* priv) {
  PyObject* rl = (PyObject*)priv;
  PyObject* m = Py_BuildValue("LLLL", a1, a2, b1, b2);
  if (!m)
    return -1;
  int r = PyList_Append(rl, m);
  Py_DECREF(m);
  return r;
}

static PyObject* blocks(PyObject* self, PyObject* args) {
  char *sa, *sb;
  int na, nb;

  if (!PyArg_ParseTuple(args, "s#s#", &sa, &na, &sb, &nb))
    return NULL;

  mmfile_t a = {sa, na}, b = {sb, nb};

  PyObject* rl = PyList_New(0);
  if (!rl)
    return PyErr_NoMemory();

  xpparam_t xpp = {
      XDF_INDENT_HEURISTIC, /* flags */
  };
  xdemitconf_t xecfg = {
      XDL_EMIT_BDIFFHUNK, /* flags */
      hunk_consumer, /* hunk_consume_func */
  };
  xdemitcb_t ecb = {
      rl, /* priv */
  };

  if (xdl_diff(&a, &b, &xpp, &xecfg, &ecb) != 0) {
    Py_DECREF(rl);
    return PyErr_NoMemory();
  }

  return rl;
}

static char xdiff_doc[] = "xdiff wrapper";

static PyMethodDef methods[] = {
    {"blocks",
     blocks,
     METH_VARARGS,
     "(a: str, b: str) -> List[(a1, a2, b1, b2)].\n"
     "Yield matched blocks. (a1, a2, b1, b2) are line numbers.\n"},
    {NULL, NULL},
};

static const int version = 1;

#ifdef IS_PY3K
static struct PyModuleDef xdiff_module = {
    PyModuleDef_HEAD_INIT,
    "xdiff",
    xdiff_doc,
    -1,
    methods,
};

PyMODINIT_FUNC PyInit_xdiff(void) {
  PyObject* m;
  m = PyModule_Create(&xdiff_module);
  PyModule_AddIntConstant(m, "version", version);
  return m;
}
#else
PyMODINIT_FUNC initxdiff(void) {
  PyObject* m;
  m = Py_InitModule3("xdiff", methods, xdiff_doc);
  PyModule_AddIntConstant(m, "version", version);
}
#endif
