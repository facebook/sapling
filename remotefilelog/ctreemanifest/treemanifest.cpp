// treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
#include <Python.h>
#include <ctype.h>
#include <iostream>
#include <string>
#include <vector>

static int8_t hextable[256] = {
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	 0,  1,  2,  3,  4,  5,  6,  7,  8,  9, -1, -1, -1, -1, -1, -1, /* 0-9 */
	-1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* A-F */
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* a-f */
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
	-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1
};

/*
 * A single instance of a treemanifest.
 * */
typedef struct {
  PyObject_HEAD;

  // A reference to the store that is used to fetch new content
  PyObject *store;

  // The 20-byte root node of this manifest
  std::string node;
} treemanifest;

/*
 * A helper struct representing the state of an iterator recursing over a tree.
 * */
typedef struct {
  PyObject *get;                // Function to fetch tree content
  std::vector<PyObject*> data;  // Tree content for previous entries in the stack
  std::vector<char*> location;    // The current iteration position for each stack entry
  std::string path;             // The fullpath for the top entry in the stack.
} stackiter;

/*
 * The python iteration object for iterating over a tree.  This is separate from
 * the stackiter above because it lets us just call the constructor on
 * stackiter, which will automatically populate all the members of stackiter.
 * */
typedef struct {
  PyObject_HEAD;
  stackiter iter;
} fileiter;

static void fileiter_dealloc(fileiter *self);
static PyTypeObject fileiterType = {
  PyObject_HEAD_INIT(NULL)
  0,                               /*ob_size */
  "treemanifest.keyiter",          /*tp_name */
  sizeof(fileiter),                /*tp_basicsize */
  0,                               /*tp_itemsize */
  (destructor)fileiter_dealloc,    /*tp_dealloc */
  0,                               /*tp_print */
  0,                               /*tp_getattr */
  0,                               /*tp_setattr */
  0,                               /*tp_compare */
  0,                               /*tp_repr */
  0,                               /*tp_as_number */
  0,                               /*tp_as_sequence */
  0,                               /*tp_as_mapping */
  0,                               /*tp_hash */
  0,                               /*tp_call */
  0,                               /*tp_str */
  0,                               /*tp_getattro */
  0,                               /*tp_setattro */
  0,                               /*tp_as_buffer */
  /* tp_flags: Py_TPFLAGS_HAVE_ITER tells python to
     use tp_iter and tp_iternext fields. */
  Py_TPFLAGS_DEFAULT | Py_TPFLAGS_HAVE_ITER,
  "TODO",                          /* tp_doc */
  0,                               /* tp_traverse */
  0,                               /* tp_clear */
  0,                               /* tp_richcompare */
  0,                               /* tp_weaklistoffset */
  PyObject_SelfIter,               /* tp_iter: __iter__() method */
  0,                               /* tp_iternext: next() method */
};

/*
 * Converts a given 20-byte node into a 40-byte hex string
 * */
static std::string binfromhex(const char *node) {
  char binary[20];
  for (int i = 0; i < 40;) {
    int hi = hextable[(unsigned char)node[i++]];
    int lo = hextable[(unsigned char)node[i++]];
    binary[(i - 2) / 2] = (hi << 4) | lo;
  }
  return std::string(binary, 20);
}

/*
 * Fetches the given directory/node pair from the store using the provided `get`
 * function. Returns a python string with the contents. The caller is
 * responsible for calling Py_DECREF on the result.
 * */
static PyObject* getdata(PyObject *get, const std::string &dir, const std::string &node) {
  PyObject *arglist, *result;

  arglist = Py_BuildValue("s#s#", dir.c_str(), dir.size(), node.c_str(), node.size());
  if (!arglist) {
    return NULL;
  }

  result = PyEval_CallObject(get, arglist);
  Py_DECREF(arglist);

  return result;
}

/*
 * Deallocates the contents of the treemanifest
 * */
static void treemanifest_dealloc(treemanifest *self){
  self->node.std::string::~string();
  Py_XDECREF(self->store);
  PyObject_Del(self);
}

/*
 * Initializes the contents of a treemanifest
 * */
static int treemanifest_init(treemanifest *self, PyObject *args) {
  PyObject *store;
  char *node;
  int nodelen;

  if (!PyArg_ParseTuple(args, "Os#", &store, &node, &nodelen)) {
    return -1;
  }

  self->store = store;
  Py_INCREF(store);
  // We have to manually call the member constructor, since the provided 'self'
  // is just zerod out memory.
  try {
    new (&self->node) std::string(node, nodelen);
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return -1;
  }

  return 0;
}

// ==== fileiter functions ====

/* Destructor for the file iterator. Cleans up all the member data of the
 * iterator.
 * */
static void fileiter_dealloc(fileiter *self) {
  Py_XDECREF(self->iter.get);
  while (self->iter.data.size() > 0) {
    Py_XDECREF(self->iter.data.back());
    self->iter.data.pop_back();
  }

  self->iter.~stackiter();
  PyObject_Del(self);
}

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
  {NULL, NULL}
};

static PyTypeObject treemanifestType = {
  PyObject_HEAD_INIT(NULL)
  0,                                                /* ob_size */
  "ctreemanifest.treemanifest",                     /* tp_name */
  sizeof(treemanifest),                             /* tp_basicsize */
  0,                                                /* tp_itemsize */
  (destructor)treemanifest_dealloc,                 /* tp_dealloc */
  0,                                                /* tp_print */
  0,                                                /* tp_getattr */
  0,                                                /* tp_setattr */
  0,                                                /* tp_compare */
  0,                                                /* tp_repr */
  0,                                                /* tp_as_number */
  0,                                                /* tp_as_sequence - length/contains */
  0,                                                /* tp_as_mapping - getitem/setitem*/
  0,                                                /* tp_hash */
  0,                                                /* tp_call */
  0,                                                /* tp_str */
  0,                                                /* tp_getattro */
  0,                                                /* tp_setattro */
  0,                                                /* tp_as_buffer */
  Py_TPFLAGS_DEFAULT,                               /* tp_flags */
  "TODO",                                           /* tp_doc */
  0,                                                /* tp_traverse */
  0,                                                /* tp_clear */
  0,                                                /* tp_richcompare */
  0,                                                /* tp_weaklistoffset */
  0,                                                /* tp_iter */
  0,                                                /* tp_iternext */
  treemanifest_methods,                             /* tp_methods */
  0,                                                /* tp_members */
  0,                                                /* tp_getset */
  0,                                                /* tp_base */
  0,                                                /* tp_dict */
  0,                                                /* tp_descr_get */
  0,                                                /* tp_descr_set */
  0,                                                /* tp_dictoffset */
  (initproc)treemanifest_init,                      /* tp_init */
  0,                                                /* tp_alloc */
};

static PyMethodDef mod_methods[] = {
  {NULL, NULL}
};

static char mod_description[] = "Module containing a native treemanifest implementation";

PyMODINIT_FUNC initctreemanifest(void)
{
  PyObject *mod;

  treemanifestType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&treemanifestType) < 0) {
    return;
  }

  mod = Py_InitModule3("ctreemanifest", mod_methods, mod_description);
  Py_INCREF(&treemanifestType);
  PyModule_AddObject(mod, "treemanifest", (PyObject *)&treemanifestType);
}
