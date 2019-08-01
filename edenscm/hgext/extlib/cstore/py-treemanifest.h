// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// py-treemanifest.cpp - c++ implementation of a tree manifest
// no-check-code
//
#ifndef FBHGEXT_CSTORE_PY_TREEMANIFEST_H
#define FBHGEXT_CSTORE_PY_TREEMANIFEST_H

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <memory>
#include <string>

#include "edenscm/hgext/extlib/cstore/py-structs.h"
#include "edenscm/hgext/extlib/cstore/pythonutil.h"
#include "edenscm/hgext/extlib/cstore/uniondatapackstore.h"
#include "edenscm/hgext/extlib/ctreemanifest/manifest.h"
#include "edenscm/hgext/extlib/ctreemanifest/treemanifest.h"
#include "lib/clib/convert.h"

#define FILENAME_BUFFER_SIZE 16348
#define FLAG_SIZE 1
#define DEFAULT_FETCH_DEPTH 65536

// clang-format off
// clang thinks that PyObject_HEAD should be on the same line as the next line
// since there is no semicolong after it. There is no semicolon because
// PyObject_HEAD macro already contains one and MSVC does not support
// extra semicolons.
struct py_treemanifest {
  PyObject_HEAD

  treemanifest tm;
};
// clang-format on

// clang-format off
struct py_newtreeiter {
  PyObject_HEAD

  FinalizeIterator iter;
};
// clang-format on

static void newtreeiter_dealloc(py_newtreeiter* self);
static PyObject* newtreeiter_iternext(py_newtreeiter* self);
static PyTypeObject newtreeiterType = {
    PyObject_HEAD_INIT(NULL) 0, /*ob_size */
    "treemanifest.newtreeiter", /*tp_name */
    sizeof(py_newtreeiter), /*tp_basicsize */
    0, /*tp_itemsize */
    (destructor)newtreeiter_dealloc, /*tp_dealloc */
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
    "TODO", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    PyObject_SelfIter, /* tp_iter: __iter__() method */
    (iternextfunc)newtreeiter_iternext, /* tp_iternext: next() method */
};

// clang-format off
struct py_subtreeiter {
  PyObject_HEAD

  SubtreeIterator iter;
};
// clang-format on

static void subtreeiter_dealloc(py_subtreeiter* self);
static PyObject* subtreeiter_iternext(py_subtreeiter* self);
static PyTypeObject subtreeiterType = {
    PyObject_HEAD_INIT(NULL) 0, /*ob_size */
    "treemanifest.subtreeiter", /*tp_name */
    sizeof(py_subtreeiter), /*tp_basicsize */
    0, /*tp_itemsize */
    (destructor)subtreeiter_dealloc, /*tp_dealloc */
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
    "TODO", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    PyObject_SelfIter, /* tp_iter: __iter__() method */
    (iternextfunc)subtreeiter_iternext, /* tp_iternext: next() method */
};
/**
 * The python iteration object for iterating over a tree.  This is separate from
 * the fileiter above because it lets us just call the constructor on
 * fileiter, which will automatically populate all the members of fileiter.
 */
// clang-format off
struct py_fileiter {
  PyObject_HEAD

  fileiter iter;

  bool includenode;
  bool includeflag;

  // A reference to the tree is kept, so it is not freed while we're iterating
  // over it.
  const py_treemanifest *treemf;
};
// clang-format on

static void fileiter_dealloc(py_fileiter* self);
static PyObject* fileiter_iterentriesnext(py_fileiter* self);
static PyTypeObject fileiterType = {
    PyObject_HEAD_INIT(NULL) 0, /*ob_size */
    "treemanifest.keyiter", /*tp_name */
    sizeof(py_fileiter), /*tp_basicsize */
    0, /*tp_itemsize */
    (destructor)fileiter_dealloc, /*tp_dealloc */
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
    "TODO", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    PyObject_SelfIter, /* tp_iter: __iter__() method */
    (iternextfunc)fileiter_iterentriesnext, /* tp_iternext: next() method */
};

static py_fileiter* createfileiter(
    py_treemanifest* pytm,
    bool includenode,
    bool includeflag,
    bool sorted,
    PythonObj matcher) {
  py_fileiter* i = PyObject_New(py_fileiter, &fileiterType);
  if (i) {
    try {
      i->treemf = pytm;
      Py_INCREF(pytm);
      i->includenode = includenode;
      i->includeflag = includeflag;

      // The provided py_fileiter struct hasn't initialized our fileiter member,
      // so we do it manually.
      new (&i->iter) fileiter(pytm->tm, sorted);
      if (matcher) {
        i->iter.matcher = std::make_shared<PythonMatcher>(matcher);
      }
      return i;
    } catch (const pyexception& ex) {
      Py_DECREF(i);
      return NULL;
    } catch (const std::exception& ex) {
      Py_DECREF(i);
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      return NULL;
    }
  } else {
    return NULL;
  }
}

static py_fileiter*
createfileiter(py_treemanifest* pytm, bool includenode, bool includeflag) {
  return createfileiter(
      pytm,
      includenode,
      includeflag,
      true, // we care about sort order.
      PythonObj());
}

// ==== py_newtreeiter functions ====

/**
 * Destructor for the newtree iterator. Cleans up all the member data of the
 * iterator.
 */
static void newtreeiter_dealloc(py_newtreeiter* self) {
  self->iter.~FinalizeIterator();
  PyObject_Del(self);
}

static py_newtreeiter* newtreeiter_create(
    ManifestPtr mainManifest,
    const std::vector<const char*>& cmpNodes,
    const std::vector<ManifestPtr>& cmpManifests,
    const ManifestFetcher& fetcher) {
  py_newtreeiter* i = PyObject_New(py_newtreeiter, &newtreeiterType);
  if (i) {
    try {
      // The provided created struct hasn't initialized our iter member, so
      // we do it manually.
      new (&i->iter)
          FinalizeIterator(mainManifest, cmpNodes, cmpManifests, fetcher);
      return i;
    } catch (const pyexception& ex) {
      Py_DECREF(i);
      return NULL;
    } catch (const std::exception& ex) {
      Py_DECREF(i);
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      return NULL;
    }
  } else {
    return NULL;
  }
}

/**
 * Returns the next new tree. If it's the final root node, it marks the tree as
 * complete and immutable.
 */
static PyObject* newtreeiter_iternext(py_newtreeiter* self) {
  FinalizeIterator& iterator = self->iter;

  std::string* path = NULL;
  ManifestPtr result = ManifestPtr();
  ManifestPtr p1 = ManifestPtr();
  ManifestPtr p2 = ManifestPtr();
  std::string raw;
  std::string p1raw;
  try {
    while (iterator.next(&path, &result, &p1, &p2)) {
      result->serialize(raw);

      if (!p1) {
        p1raw.erase();
      } else {
        p1->serialize(p1raw);
      }

      const char* p1Node = p1 ? p1->node() : NULLID;
      const char* p2Node = p2 ? p2->node() : NULLID;
      return Py_BuildValue(
          "(s#s#s#s#s#s#)",
          path->c_str(),
          (Py_ssize_t)path->size(),
          result->node(),
          (Py_ssize_t)BIN_NODE_SIZE,
          raw.c_str(),
          (Py_ssize_t)raw.size(),
          p1raw.c_str(),
          (Py_ssize_t)p1raw.size(),
          p1Node,
          (Py_ssize_t)BIN_NODE_SIZE,
          p2Node,
          (Py_ssize_t)BIN_NODE_SIZE);
    }
  } catch (const pyexception&) {
    return NULL;
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return NULL;
}

// ==== py_subtreeiter functions ====

/**
 * Destructor for the subtree iterator. Cleans up all the member data of the
 * iterator.
 */
static void subtreeiter_dealloc(py_subtreeiter* self) {
  self->iter.~SubtreeIterator();
  PyObject_Del(self);
}

static py_subtreeiter* subtreeiter_create(
    std::string& path,
    ManifestPtr mainManifest,
    const std::vector<ManifestPtr>& cmpManifests,
    const ManifestFetcher& fetcher,
    const int depth) {
  py_subtreeiter* pyiter = PyObject_New(py_subtreeiter, &subtreeiterType);
  if (pyiter) {
    try {
      // The provided created struct hasn't initialized our iter member, so
      // we do it manually.
      std::vector<const char*> cmpNodes(cmpManifests.size());
      for (size_t i = 0; i < cmpManifests.size(); ++i) {
        cmpNodes.push_back(cmpManifests[i]->node());
      }
      new (&pyiter->iter) SubtreeIterator(
          path, mainManifest, cmpNodes, cmpManifests, fetcher, depth);
      return pyiter;
    } catch (const pyexception& ex) {
      PyObject_Del(pyiter);
      return NULL;
    } catch (const std::exception& ex) {
      PyObject_Del(pyiter);
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      return NULL;
    }
  } else {
    return NULL;
  }
}

/**
 * Returns the next new tree. If it's the final root node, it marks the tree as
 * complete and immutable.
 */
static PyObject* subtreeiter_iternext(py_subtreeiter* self) {
  SubtreeIterator& iterator = self->iter;

  std::string* path = NULL;
  ManifestPtr result = ManifestPtr();
  ManifestPtr p1 = ManifestPtr();
  ManifestPtr p2 = ManifestPtr();
  std::string raw;
  std::string p1raw;
  try {
    while (iterator.next(&path, &result, &p1, &p2)) {
      result->serialize(raw);

      if (!p1) {
        p1raw.erase();
      } else {
        p1->serialize(p1raw);
      }

      const char* p1Node = p1 ? p1->node() : NULLID;
      const char* p2Node = p2 ? p2->node() : NULLID;
      return Py_BuildValue(
          "(s#s#s#s#s#s#)",
          path->c_str(),
          (Py_ssize_t)path->size(),
          result->node(),
          (Py_ssize_t)BIN_NODE_SIZE,
          raw.c_str(),
          (Py_ssize_t)raw.size(),
          p1raw.c_str(),
          (Py_ssize_t)p1raw.size(),
          p1Node,
          (Py_ssize_t)BIN_NODE_SIZE,
          p2Node,
          (Py_ssize_t)BIN_NODE_SIZE);
    }
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return NULL;
}

// ==== treemanifest functions ====

/**
 * Implementation of treemanifest.__iter__
 * Returns a PyObject iterator instance.
 */
static PyObject* treemanifest_getkeysiter(py_treemanifest* self) {
  return (PyObject*)createfileiter(self, false, false);
}

static PyObject* treemanifest_keys(py_treemanifest* self) {
  PythonObj iter = (PyObject*)createfileiter(self, false, false);
  PythonObj args = Py_BuildValue("(O)", (PyObject*)iter);
  PyObject* result =
      PyEval_CallObject((PyObject*)&PyList_Type, (PyObject*)args);
  return result;
}

static PyObject* treemanifest_dirs(py_treemanifest* self) {
  PythonObj module = PyImport_ImportModule("edenscm.mercurial.util");
  PythonObj dirstype = module.getattr("dirs");

  PyObject* args = Py_BuildValue("(O)", self);
  PythonObj result = dirstype.call(args);
  return result.returnval();
}

static PyObject*
treemanifest_diff(PyObject* o, PyObject* args, PyObject* kwargs) {
  py_treemanifest* self = (py_treemanifest*)o;
  PyObject* otherObj;
  PyObject* matcherObj = NULL;
  PyObject* cleanObj = NULL;
  static char const* kwlist[] = {"m2", "matcher", "clean", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args,
          kwargs,
          "O|OO",
          (char**)kwlist,
          &otherObj,
          &matcherObj,
          &cleanObj)) {
    return NULL;
  }

  py_treemanifest* other = (py_treemanifest*)otherObj;

  PythonObj matcher;
  if (matcherObj && matcherObj != Py_None) {
    matcher = matcherObj;
    Py_INCREF(matcherObj);
  }

  PythonMatcher pythonMatcher(matcher);
  AlwaysMatcher alwaysMatcher;
  Matcher* matcherPtr = &alwaysMatcher;
  if (matcher) {
    matcherPtr = &pythonMatcher;
  }

  bool clean = false;
  if (cleanObj && PyObject_IsTrue(cleanObj)) {
    clean = true;
  }

  PythonDiffResult results(PyDict_New());

  ManifestFetcher fetcher = self->tm.fetcher;

  std::string path;
  try {
    path.reserve(1024);

    treemanifest_diffrecurse(
        self->tm.getRootManifest(),
        other->tm.getRootManifest(),
        path,
        results,
        fetcher,
        clean,
        *matcherPtr);
  } catch (const pyexception& ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return results.getDiff().returnval();
}

static PyObject*
treemanifest_get(py_treemanifest* self, PyObject* args, PyObject* kwargs) {
  char* filename;
  Py_ssize_t filenamelen;

  PyObject* defaultObj = NULL;
  static char const* kwlist[] = {"key", "default", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args,
          kwargs,
          "s#|O",
          (char**)kwlist,
          &filename,
          &filenamelen,
          &defaultObj)) {
    return NULL;
  }

  std::string resultnode;
  const char* resultflag;
  bool found;
  try {
    found = self->tm.get(
        std::string(filename, (size_t)filenamelen), &resultnode, &resultflag);
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (!found) {
    if (PyErr_Occurred()) {
      return NULL;
    }

    if (defaultObj) {
      Py_INCREF(defaultObj);
      return defaultObj;
    }
    Py_RETURN_NONE;
  } else {
    return Py_BuildValue(
        "s#", resultnode.c_str(), (Py_ssize_t)resultnode.length());
  }
}

static PyObject* treemanifest_hasdir(py_treemanifest* self, PyObject* args) {
  char* directory;
  Py_ssize_t directorylen;

  if (!PyArg_ParseTuple(args, "s#", &directory, &directorylen)) {
    return NULL;
  }

  std::string directorystr(directory, directorylen);

  std::string resultnode;
  const char* resultflag = NULL;
  bool found;
  try {
    found =
        self->tm.get(directorystr, &resultnode, &resultflag, RESULT_DIRECTORY);
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (found && resultflag && *resultflag == MANIFEST_DIRECTORY_FLAG) {
    Py_RETURN_TRUE;
  } else {
    Py_RETURN_FALSE;
  }
}

/**
 * Implementation of treemanifest.listdir
 * Takes a directory name and returns a list of files and directories in
 * that directory.  If the directory doesn't exist, or is a file, returns
 * None.
 */
static PyObject* treemanifest_listdir(py_treemanifest* self, PyObject* args) {
  char* directory;
  Py_ssize_t directorylen;

  if (!PyArg_ParseTuple(args, "s#", &directory, &directorylen)) {
    return NULL;
  }

  std::string directorystr(directory, directorylen);
  ManifestPtr manifest;

  if (directorystr.empty()) {
    manifest = self->tm.getRootManifest();
  } else {
    std::string resultnode;
    const char* resultflag = NULL;
    try {
      self->tm.get(
          directorystr, &resultnode, &resultflag, RESULT_DIRECTORY, &manifest);
    } catch (const pyexception& ex) {
      return NULL;
    }
  }

  if (manifest) {
    PyObject* files = PyList_New(manifest->children());
    Py_ssize_t i = 0;
    for (ManifestIterator iterator = manifest->getIterator();
         !iterator.isfinished();
         iterator.next()) {
      ManifestEntry* entry = iterator.currentvalue();
      PyList_SetItem(
          files,
          i++,
          PyString_FromStringAndSize(entry->filename, entry->filenamelen));
    }
    return files;
  } else {
    Py_RETURN_NONE;
  }
}

/**
 * Implementation of treemanifest.find()
 * Takes a filename and returns a tuple of the binary hash and flag,
 * or (None, None) if it doesn't exist.
 */
static PyObject* treemanifest_find(PyObject* o, PyObject* args) {
  py_treemanifest* self = (py_treemanifest*)o;
  char* filename;
  Py_ssize_t filenamelen;

  if (!PyArg_ParseTuple(args, "s#", &filename, &filenamelen)) {
    return NULL;
  }

  std::string resultnode;
  const char* resultflag;
  bool found;
  try {
    // Grab the root node's data

    found = self->tm.get(
        std::string(filename, (size_t)filenamelen), &resultnode, &resultflag);
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (!found) {
    if (!PyErr_Occurred()) {
      PyErr_Format(
          PyExc_KeyError, "cannot find file '%s' in manifest", filename);
    }
    return NULL;
  } else {
    Py_ssize_t flaglen;
    if (resultflag == NULL) {
      flaglen = 0;
      resultflag = MAGIC_EMPTY_STRING;
    } else {
      flaglen = 1;
    }
    return Py_BuildValue(
        "s#s#",
        resultnode.c_str(),
        (Py_ssize_t)resultnode.length(),
        resultflag,
        flaglen);
  }
}

/**
 * Implementation of treemanifest.set()
 * Takes a binary hash and flag and sets it for a given filename.
 */
static PyObject* treemanifest_set(PyObject* o, PyObject* args) {
  py_treemanifest* self = (py_treemanifest*)o;
  char* filename;
  Py_ssize_t filenamelen;
  char* hash;
  Py_ssize_t hashlen;
  char* flagstr;
  Py_ssize_t flagstrlen;
  const char* flag;

  if (!PyArg_ParseTuple(
          args,
          "s#z#z#",
          &filename,
          &filenamelen,
          &hash,
          &hashlen,
          &flagstr,
          &flagstrlen)) {
    return NULL;
  }

  // verify that the lengths of the fields are sane.
  if (hash == NULL && flagstr == NULL) {
    // this is a remove operation!!
    self->tm.remove(std::string(filename, (size_t)filenamelen));
    Py_RETURN_NONE;
  } else if (hashlen != (ssize_t)BIN_NODE_SIZE) {
    PyErr_Format(
        PyExc_ValueError, "hash length must be %zu bytes long", BIN_NODE_SIZE);
    return NULL;
  } else if (flagstrlen > 1) {
    PyErr_Format(PyExc_ValueError, "flags must either be 0 or 1 byte long");
    return NULL;
  }

  if (flagstrlen == 0) {
    flag = NULL;
  } else {
    flag = flagstr;
  }

  try {
    std::string hashstr;
    hashstr.reserve(HEX_NODE_SIZE);
    hexfrombin(hash, hashstr);

    SetResult result =
        self->tm.set(std::string(filename, (size_t)filenamelen), hashstr, flag);

    if (result == SET_OK) {
      Py_RETURN_NONE;
    } else {
      PyErr_Format(PyExc_TypeError, "unexpected stuff happened");
      return NULL;
    }
  } catch (const pyexception& ex) {
    return NULL;
  }
}

static PyObject* treemanifest_setflag(PyObject* o, PyObject* args) {
  py_treemanifest* self = (py_treemanifest*)o;
  char* filename;
  Py_ssize_t filenamelen;
  char* flag;
  Py_ssize_t flaglen;

  if (!PyArg_ParseTuple(
          args, "s#s#", &filename, &filenamelen, &flag, &flaglen)) {
    return NULL;
  }

  std::string filenamestr(filename, filenamelen);

  // Get the current node so we don't overwrite it
  std::string existingnode;
  const char* existingflag = NULL;
  try {
    std::string existingbinnode;
    self->tm.get(filenamestr, &existingbinnode, &existingflag);
    if (!existingbinnode.empty()) {
      hexfrombin(existingbinnode.c_str(), existingnode);
    }
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (existingnode.empty()) {
    PyErr_Format(
        PyExc_KeyError, "cannot setflag on file that is not in manifest");
    return NULL;
  }

  try {
    if (!flaglen) {
      flag = NULL;
    }
    SetResult result = self->tm.set(filenamestr, existingnode, flag);

    if (result == SET_OK) {
      Py_RETURN_NONE;
    } else {
      PyErr_Format(PyExc_TypeError, "unexpected error during setitem");
      return NULL;
    }
  } catch (const pyexception& ex) {
    return NULL;
  }
}

/*
 * Deallocates the contents of the treemanifest
 */
static void treemanifest_dealloc(py_treemanifest* self) {
  self->tm.~treemanifest();
  PyObject_Del(self);
}

static std::shared_ptr<Store> convert_pystore(PythonObj storeObj) {
  PythonObj cstoreModule = PyImport_ImportModule("edenscmnative.cstore");
  PythonObj unionStoreType = cstoreModule.getattr("uniondatapackstore");

  // If it's a cstore, we'll use it directly instead of through python.
  std::shared_ptr<Store> store;
  int isinstance =
      PyObject_IsInstance((PyObject*)storeObj, (PyObject*)unionStoreType);
  if (isinstance == 1) {
    store = ((py_uniondatapackstore*)(PyObject*)storeObj)->uniondatapackstore;
  }

  if (!store) {
    store = std::make_shared<PythonStore>(storeObj);
  }

  return store;
}

static void
convert_pykey(PythonObj key, char** path, size_t* pathlen, std::string* node) {
  PyObject* pathObj = PyTuple_GetItem((PyObject*)key, 0);
  PyObject* nodeObj = PyTuple_GetItem((PyObject*)key, 1);

  Py_ssize_t pyPathlen;
  if (PyString_AsStringAndSize(pathObj, path, &pyPathlen)) {
    throw pyexception();
  }
  *pathlen = pyPathlen;

  char* nodedata;
  Py_ssize_t nodedatalen;
  if (PyString_AsStringAndSize(nodeObj, &nodedata, &nodedatalen)) {
    throw pyexception();
  }

  *node = std::string(nodedata, (size_t)nodedatalen);
}

/*
 * Initializes the contents of a treemanifest
 */
static int treemanifest_init(py_treemanifest* self, PyObject* args) {
  PyObject* pystore;
  char* node = NULL;
  Py_ssize_t nodelen;

  if (!PyArg_ParseTuple(args, "O|s#", &pystore, &node, &nodelen)) {
    return -1;
  }

  Py_INCREF(pystore);
  PythonObj storeObj = PythonObj(pystore);

  auto store = convert_pystore(storeObj);

  // We have to manually call the member constructor, since the provided 'self'
  // is just zerod out memory.
  try {
    if (node != NULL) {
      new (&self->tm) treemanifest(store, std::string(node, (size_t)nodelen));
    } else {
      new (&self->tm) treemanifest(store);
    }
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return -1;
  }

  return 0;
}

// ==== py_fileiter functions ====

/**
 * Destructor for the file iterator. Cleans up all the member data of the
 * iterator.
 */
static void fileiter_dealloc(py_fileiter* self) {
  self->iter.~fileiter();
  Py_XDECREF(self->treemf);
  PyObject_Del(self);
}

/**
 * Pops the data and location entries on the iter stack, for all stack entries
 * that we've already fully processed.
 *
 * Returns false if we've reached the end, or true if there's more work.
 */
static bool fileiter_popfinished(fileiter* iter) {
  stackframe* frame = &iter->frames.back();

  // Pop the stack of trees until we find one we haven't finished iterating
  // over.
  while (frame->isfinished()) {
    iter->frames.pop_back();
    if (iter->frames.empty()) {
      // No more directories to pop means we've reached the end of the root
      return false;
    }
    frame = &iter->frames.back();

    // Pop the top of the path off, to match the newly popped tree stack.
    size_t found = iter->path.rfind('/', iter->path.size() - 2);
    if (found != std::string::npos) {
      iter->path.erase(found + 1);
    } else {
      iter->path.erase(size_t(0));
    }
  }

  return true;
}

/**
 * Moves the given iterator to the next file in the manifest.
 * `path` - a character array with length `pathcapacity`
 * `node` - a character array with length 20
 * `flag` - a character array with length 1
 *
 * If the function returns true, the provided buffers have been filled in with
 * path, node and flag data. The path field is null terminated. If there is no
 * flag, the flag array is set to ['\0'].
 *
 * If the function return false, the buffers are left alone and we've reached
 * the end of the iterator.
 */
static bool fileiter_next(
    fileiter& iter,
    char* path,
    size_t pathcapacity,
    char* node,
    char* flag) {
  // Iterate over the current directory contents
  while (true) {
    // Pop off any directories that we're done processing
    if (!fileiter_popfinished(&iter)) {
      // No more directories means we've reached the end of the root
      return false;
    }

    stackframe& frame = iter.frames.back();

    ManifestEntry* entry;
    entry = frame.next();

    // If a directory, push it and loop again
    if (entry->isdirectory()) {
      iter.path.append(entry->filename, entry->filenamelen);

      // Check if we should visit the directory
      if (iter.matcher && !iter.matcher->visitdir(iter.path)) {
        iter.path.erase(iter.path.size() - entry->filenamelen);
        continue;
      }

      iter.path.append(1, '/');

      Manifest* submanifest = entry->get_manifest(
          iter.fetcher, iter.path.c_str(), iter.path.size());

      // TODO: memory cleanup here is probably broken.
      iter.frames.push_back(stackframe(submanifest, iter.sorted));
    } else {
      // If a file, yield it
      if (iter.path.size() + entry->filenamelen + 1 > pathcapacity) {
        throw std::logic_error("filename too long for buffer");
      }

      iter.path.copy(path, iter.path.size());
      strncpy(path + iter.path.size(), entry->filename, entry->filenamelen);

      size_t pathlen = iter.path.size() + entry->filenamelen;
      path[pathlen] = '\0';

      if (iter.matcher && !iter.matcher->matches(path, pathlen)) {
        continue;
      }

      std::string binnode = binfromhex(entry->get_node());
      binnode.copy(node, BIN_NODE_SIZE);
      if (entry->flag) {
        *flag = *entry->flag;
      } else {
        *flag = '\0';
      }
      return true;
    }
  }
}

/**
 * Returns the next object in the iteration.
 */
static PyObject* fileiter_iterentriesnext(py_fileiter* self) {
  fileiter& iter = self->iter;

  try {
    char path[FILENAME_BUFFER_SIZE];
    char node[BIN_NODE_SIZE];
    char flag[FLAG_SIZE];
    if (fileiter_next(iter, path, FILENAME_BUFFER_SIZE, node, flag)) {
      if (self->includenode && self->includeflag) {
        size_t flaglen = 0;
        if (flag[0] != '\0') {
          flaglen = 1;
        }
        return Py_BuildValue(
            "(s#s#s#)", path, strlen(path), node, BIN_NODE_SIZE, flag, flaglen);
      }
      if (self->includenode) {
        return Py_BuildValue("(s#s#)", path, strlen(path), node, BIN_NODE_SIZE);
      }
      if (self->includeflag) {
        size_t flaglen = 0;
        if (flag[0] != '\0') {
          flaglen = 1;
        }
        return Py_BuildValue("(s#s#)", path, strlen(path), flag, flaglen);
      } else {
        return PyString_FromStringAndSize(path, strlen(path));
      }
    }
    return NULL;
  } catch (const pyexception& ex) {
    return NULL;
  }
}

/**
 * Implements treemanifest.__getitem__(path)
 * Returns the node of the given file.
 */
static PyObject* treemanifest_getitem(py_treemanifest* self, PyObject* key) {
  char* filename;
  Py_ssize_t filenamelen;
  if (PyString_AsStringAndSize(key, &filename, &filenamelen)) {
    return NULL;
  }

  std::string resultnode;
  const char* resultflag;
  bool found;
  try {
    found = self->tm.get(
        std::string(filename, (size_t)filenamelen), &resultnode, &resultflag);
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (!found) {
    if (PyErr_Occurred()) {
      return NULL;
    }

    PyErr_Format(PyExc_KeyError, "file '%s' not found", filename);
    return NULL;
  } else {
    return Py_BuildValue(
        "s#", resultnode.c_str(), (Py_ssize_t)resultnode.length());
  }
}

static int
treemanifest_setitem(py_treemanifest* self, PyObject* key, PyObject* value) {
  char* filename;
  Py_ssize_t filenamelen;
  if (PyString_AsStringAndSize(key, &filename, &filenamelen)) {
    return -1;
  }
  std::string filenamestr(filename, filenamelen);

  if (!value) {
    // No value means a delete operation
    try {
      self->tm.remove(std::string(filename, (size_t)filenamelen));
      return 0;
    } catch (const pyexception& ex) {
      return -1;
    }
  }

  char* node;
  Py_ssize_t nodelen;
  if (PyString_AsStringAndSize(value, &node, &nodelen)) {
    return -1;
  }

  if (nodelen != (ssize_t)BIN_NODE_SIZE) {
    PyErr_Format(PyExc_ValueError, "invalid node length %zd", nodelen);
    return -1;
  }

  // Get the current flag so we don't overwrite it
  std::string existingnode;
  const char* existingflag = NULL;
  try {
    self->tm.get(filenamestr, &existingnode, &existingflag);
  } catch (const pyexception& ex) {
    return -1;
  }

  try {
    std::string hashstr;
    hashstr.reserve(HEX_NODE_SIZE);
    hexfrombin(node, hashstr);

    SetResult result = self->tm.set(filenamestr, hashstr, existingflag);

    if (result == SET_OK) {
      return 0;
    } else {
      PyErr_Format(PyExc_TypeError, "unexpected error during setitem");
      return -1;
    }
  } catch (const pyexception& ex) {
    return -1;
  }
}

/**
 * Implements treemanifest.flags(path)
 * Returns the flag of the given file.
 */
static PyObject*
treemanifest_flags(py_treemanifest* self, PyObject* args, PyObject* kwargs) {
  char* filename;
  Py_ssize_t filenamelen;
  char* defaultval = NULL;
  Py_ssize_t defaultvallen;
  static char const* kwlist[] = {"key", "default", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args,
          kwargs,
          "s#|s#",
          (char**)kwlist,
          &filename,
          &filenamelen,
          &defaultval,
          &defaultvallen)) {
    return NULL;
  }

  std::string resultnode;
  const char* resultflag = NULL;
  bool found;
  try {
    found = self->tm.get(
        std::string(filename, (size_t)filenamelen), &resultnode, &resultflag);
  } catch (const pyexception& ex) {
    return NULL;
  }

  if (!found) {
    if (defaultval) {
      return PyString_FromStringAndSize(defaultval, defaultvallen);
    } else {
      return PyString_FromStringAndSize(MAGIC_EMPTY_STRING, (Py_ssize_t)0);
    }
  } else {
    if (resultflag) {
      return PyString_FromStringAndSize(resultflag, (Py_ssize_t)1);
    } else {
      return PyString_FromStringAndSize(MAGIC_EMPTY_STRING, (Py_ssize_t)0);
    }
  }
}

static PyObject* treemanifest_copy(py_treemanifest* self) {
  PythonObj module = PyImport_ImportModule("edenscmnative.cstore");
  PythonObj treetype = module.getattr("treemanifest");
  py_treemanifest* copy =
      PyObject_New(py_treemanifest, (PyTypeObject*)(PyObject*)treetype);
  PythonObj copyObj((PyObject*)copy);

  new (&copy->tm) treemanifest(self->tm);

  return copyObj.returnval();
}

/**
 * Returns true if we can take the fast path for the given matcher.
 * The fastpath is for when the matcher contains a small list of specific file
 * names, so we can test each file instead of iterating over the whole manifest.
 */
static bool canusematchfastpath(py_treemanifest* /*self*/, PythonObj matcher) {
  PythonObj emptyargs = PyTuple_New(0);
  PythonObj files = matcher.callmethod("files", emptyargs);

  Py_ssize_t length = PyList_Size(files);
  if (length > 100) {
    return false;
  }

  if (!PyObject_IsTrue(matcher.callmethod("isexact", emptyargs))) {
    // TODO: the python version of this function also allows the fastpath when
    //       (match.prefix() and all(fn in self for fn in files)))
    return false;
  }

  return true;
}

/**
 * Uses the fast path to test the matcher against the tree. The fast path
 * iterates over the files in the matcher, instead of iterating over the entire
 * manifest.
 */
static PyObject* treemanifest_matchesfastpath(
    py_treemanifest* self,
    PythonObj matcher) {
  PythonObj emptyargs = PyTuple_New(0);
  PythonObj manifestmod = PyImport_ImportModule("edenscm.mercurial.manifest");
  PythonObj manifestdict = manifestmod.getattr("manifestdict");
  PythonObj result = manifestdict.call(emptyargs);

  PythonObj files = matcher.callmethod("files", emptyargs);

  std::string pathstring;
  std::string resultnode;

  PythonObj iterator = PyObject_GetIter((PyObject*)files);
  PyObject* fileObj;
  PythonObj file;
  while ((fileObj = PyIter_Next(iterator))) {
    file = fileObj;
    char* path;
    Py_ssize_t pathlen;
    if (PyString_AsStringAndSize(file, &path, &pathlen)) {
      throw pyexception();
    }

    const char* resultflag = NULL;

    pathstring.assign(path, (size_t)pathlen);
    if (!self->tm.get(path, &resultnode, &resultflag)) {
      continue;
    }

    // Call manifestdict.__setitem__
    PythonObj setArgs =
        Py_BuildValue("s#s#", path, pathlen, resultnode.c_str(), BIN_NODE_SIZE);
    result.callmethod("__setitem__", setArgs);

    Py_ssize_t flaglen;
    if (!resultflag) {
      flaglen = 0;
      resultflag = MAGIC_EMPTY_STRING;
    } else {
      flaglen = 1;
    }
    PythonObj flagArgs =
        Py_BuildValue("s#s#", path, pathlen, resultflag, flaglen);
    result.callmethod("setflag", flagArgs);
  }

  if (PyErr_Occurred()) {
    throw pyexception();
  }

  return result.returnval();
}

static PyObject* treemanifest_matches(py_treemanifest* self, PyObject* args) {
  PyObject* matcherObj;

  if (!PyArg_ParseTuple(args, "O", &matcherObj)) {
    return NULL;
  }
  // ParseTuple doesn't increment the ref, but the PythonObj will decrement on
  // destruct, so let's increment now.
  Py_INCREF(matcherObj);
  PythonObj matcher = matcherObj;

  PythonObj emptyargs = PyTuple_New(0);
  if (PyObject_IsTrue(matcher.callmethod("always", emptyargs))) {
    return treemanifest_copy(self);
  }

  try {
    // If the matcher is a list of files, take the fastpath
    if (canusematchfastpath(self, matcher)) {
      return treemanifest_matchesfastpath(self, matcher);
    }

    PythonObj manifestmod = PyImport_ImportModule("edenscm.mercurial.manifest");
    PythonObj manifestdict = manifestmod.getattr("manifestdict");
    PythonObj result = manifestdict.call(emptyargs);

    fileiter iter = fileiter(self->tm, false);
    if (matcher) {
      iter.matcher = std::make_shared<PythonMatcher>(matcher);
    }

    char path[FILENAME_BUFFER_SIZE];
    char node[BIN_NODE_SIZE];
    char flag[1];
    while (fileiter_next(iter, path, FILENAME_BUFFER_SIZE, node, flag)) {
      size_t pathlen = strlen(path);

      // Call manifestdict.__setitem__
      PythonObj setArgs =
          Py_BuildValue("s#s#", path, pathlen, node, BIN_NODE_SIZE);
      result.callmethod("__setitem__", setArgs);

      Py_ssize_t flaglen = *flag != '\0' ? 1 : 0;
      PythonObj flagArgs = Py_BuildValue("s#s#", path, pathlen, flag, flaglen);
      result.callmethod("setflag", flagArgs);
    }

    return result.returnval();
  } catch (const pyexception& ex) {
    return NULL;
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return NULL;
}

static PyObject* treemanifest_filesnotin(
    py_treemanifest* self,
    PyObject* args,
    PyObject* kwargs) {
  py_treemanifest* other;
  PyObject* matcherObj = NULL;

  static char const* kwlist[] = {"m2", "matcher", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args, kwargs, "O|O", (char**)kwlist, &other, &matcherObj)) {
    return NULL;
  }

  PythonDiffResult diffresults(PyDict_New());

  ManifestFetcher fetcher = self->tm.fetcher;

  PythonObj matcher;
  if (matcherObj && matcherObj != Py_None) {
    matcher = matcherObj;
    Py_INCREF(matcherObj);
  }

  PythonMatcher pythonMatcher(matcher);
  AlwaysMatcher alwaysMatcher;
  Matcher* matcherPtr = &alwaysMatcher;
  if (matcher) {
    matcherPtr = &pythonMatcher;
  }

  std::string path;
  try {
    path.reserve(1024);
    treemanifest_diffrecurse(
        self->tm.getRootManifest(),
        other->tm.getRootManifest(),
        path,
        diffresults,
        fetcher,
        /*clean=*/false,
        *matcherPtr);
  } catch (const pyexception& ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  PythonObj result = PySet_New(NULL);

  // All of the PyObjects below are borrowed references, so no ref counting is
  // required.
  Py_ssize_t iterpos = 0;
  PyObject* pathkey;
  PyObject* diffentry;
  PythonObj diff = diffresults.getDiff();
  while (PyDict_Next(diff, &iterpos, &pathkey, &diffentry)) {
    // Each value is a `((m1node, m1flag), (m2node, m2flag))` tuple.
    // If m2node is None, then this file doesn't exist in m2.
    PyObject* targetvalue = PyTuple_GetItem(diffentry, 1);
    if (!targetvalue) {
      return NULL;
    }
    PyObject* targetnode = PyTuple_GetItem(targetvalue, 0);
    if (!targetnode) {
      return NULL;
    }
    if (targetnode == Py_None) {
      PySet_Add(result, pathkey);
    }
  }

  return result.returnval();
}

static int treemanifest_contains(py_treemanifest* self, PyObject* key) {
  if (key == Py_None) {
    return 0;
  }

  char* filename;
  Py_ssize_t filenamelen;
  if (PyString_AsStringAndSize(key, &filename, &filenamelen)) {
    return -1;
  }

  std::string resultnode;
  const char* resultflag;
  try {
    bool found = self->tm.get(
        std::string(filename, (size_t)filenamelen), &resultnode, &resultflag);
    if (!found) {
      return 0;
    } else {
      return 1;
    }
  } catch (const pyexception& ex) {
    return -1;
  }
}

static PyObject* treemanifest_getentriesiter(py_treemanifest* self) {
  return (PyObject*)createfileiter(self, true, true);
}

static PyObject* treemanifest_iteritems(py_treemanifest* self) {
  return (PyObject*)createfileiter(self, true, false);
}

static PyObject*
treemanifest_text(py_treemanifest* self, PyObject* args, PyObject* kwargs) {
  try {
    std::string result;
    result.reserve(150 * 1024 * 1024);

    fileiter iter = fileiter(self->tm, true);

    char path[FILENAME_BUFFER_SIZE];
    char node[BIN_NODE_SIZE];
    char flag[1];
    while (fileiter_next(iter, path, FILENAME_BUFFER_SIZE, node, flag)) {
      result.append(path, strlen(path));
      result.append(1, '\0');
      hexfrombin(node, result);

      size_t flaglen = flag[0] != '\0' ? 1 : 0;
      result.append(flag, flaglen);
      result.append(1, '\n');
    }

    return PyString_FromStringAndSize(result.c_str(), result.size());
  } catch (const pyexception& ex) {
    return NULL;
  }
}

static PyObject* treemanifest_walksubtrees(
    py_treemanifest* self,
    PyObject* args,
    PyObject* kwargs) {
  PyObject* compareTrees = NULL;
  static char const* kwlist[] = {"comparetrees", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args, kwargs, "|O", (char**)kwlist, &compareTrees)) {
    return NULL;
  }

  try {
    std::vector<ManifestPtr> cmpManifests;

    if (compareTrees) {
      PythonObj iterator = PyObject_GetIter(compareTrees);
      PyObject* pyCompareTreeObj;
      while ((pyCompareTreeObj = PyIter_Next(iterator))) {
        // Assign to PythonObj so its lifecycle is managed.
        PythonObj pyCompareTree = pyCompareTreeObj;
        py_treemanifest* compareTree = (py_treemanifest*)pyCompareTreeObj;
        cmpManifests.push_back(compareTree->tm.getRootManifest());
      }
    }

    auto rootPath = std::string("");
    auto depth = DEFAULT_FETCH_DEPTH;
    return (PyObject*)subtreeiter_create(
        rootPath,
        self->tm.getRootManifest(),
        cmpManifests,
        self->tm.fetcher,
        depth);
  } catch (const pyexception& ex) {
    return NULL;
  }
}

static PyObject* treemanifest_walksubdirtrees(
    PyTypeObject* type,
    PyObject* args,
    PyObject* kwargs) {
  PyObject* keyObj = NULL;
  PyObject* storeObj = NULL;
  PyObject* compareTrees = NULL;
  int depth = DEFAULT_FETCH_DEPTH;
  static char const* kwlist[] = {"key", "store", "comparetrees", "depth", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args,
          kwargs,
          "OO|Oi",
          (char**)kwlist,
          &keyObj,
          &storeObj,
          &compareTrees,
          &depth)) {
    return NULL;
  }

  try {
    Py_INCREF(storeObj);
    PythonObj store = storeObj;
    Py_INCREF(keyObj);
    PythonObj key = keyObj;

    char* path;
    size_t pathlen;
    std::string node;
    convert_pykey(key, &path, &pathlen, &node);

    // Get the manifest
    auto fetcher = ManifestFetcher(convert_pystore(store));
    auto manifest = fetcher.get(path, pathlen, node);

    std::vector<ManifestPtr> cmpManifests;

    if (compareTrees) {
      PythonObj iterator = PyObject_GetIter(compareTrees);
      PyObject* compareKeyObj;
      while ((compareKeyObj = PyIter_Next(iterator))) {
        // Assign to PythonObj so its lifecycle is managed.
        PythonObj compareKey = compareKeyObj;

        char* cmpPath;
        size_t cmpPathlen;
        std::string cmpNode;
        convert_pykey(compareKey, &cmpPath, &cmpPathlen, &cmpNode);

        auto cmpManifest = fetcher.get(cmpPath, cmpPathlen, cmpNode);
        cmpManifests.push_back(cmpManifest);
      }
    }

    auto pathStr = std::string(path, pathlen);
    return (PyObject*)subtreeiter_create(
        pathStr, manifest, cmpManifests, fetcher, depth);
  } catch (const pyexception& ex) {
    return NULL;
  } catch (const std::exception& ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }
}

static PyObject* treemanifest_walk(py_treemanifest* self, PyObject* args) {
  PyObject* matcherObj;

  if (!PyArg_ParseTuple(args, "O", &matcherObj)) {
    return NULL;
  }
  // ParseTuple doesn't increment the ref, but the PythonObj will decrement on
  // destruct, so let's increment now.
  Py_INCREF(matcherObj);
  PythonObj matcher = matcherObj;

  return (PyObject*)createfileiter(
      self,
      false,
      false,
      false, // walk does not care about sort order.
      matcher);
}

static PyObject*
treemanifest_finalize(py_treemanifest* self, PyObject* args, PyObject* kwargs) {
  PyObject* p1treeObj = NULL;
  PyObject* p2treeObj = NULL;

  static char const* kwlist[] = {"p1tree", "p2tree", NULL};

  if (!PyArg_ParseTupleAndKeywords(
          args, kwargs, "|OO", (char**)kwlist, &p1treeObj, &p2treeObj)) {
    return NULL;
  }

  py_treemanifest* p1tree = NULL;
  if (p1treeObj && p1treeObj != Py_None) {
    p1tree = (py_treemanifest*)p1treeObj;
  }

  py_treemanifest* p2tree = NULL;
  if (p2treeObj && p2treeObj != Py_None) {
    p2tree = (py_treemanifest*)p2treeObj;
  }

  try {
    std::vector<const char*> cmpNodes;
    std::vector<ManifestPtr> cmpManifests;
    if (p1tree) {
      assert(p1tree->tm.root.get_node());
      cmpNodes.push_back(p1tree->tm.root.get_node());
      cmpManifests.push_back(p1tree->tm.getRootManifest());
    }
    if (p2tree) {
      assert(p2tree->tm.root.get_node());
      cmpNodes.push_back(p2tree->tm.root.get_node());
      cmpManifests.push_back(p2tree->tm.getRootManifest());
    }

    return (PyObject*)newtreeiter_create(
        self->tm.getRootManifest(), cmpNodes, cmpManifests, self->tm.fetcher);
  } catch (const pyexception& ex) {
    return NULL;
  }
}

static int treemanifest_nonzero(py_treemanifest* self) {
  try {
    if (self->tm.getRootManifest()->children() > 0) {
      return 1;
    } else {
      return 0;
    }
  } catch (const pyexception& ex) {
    return -1;
  }
}

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
    {"copy",
     (PyCFunction)treemanifest_copy,
     METH_NOARGS,
     "copies the treemanifest"},
    {"diff",
     (PyCFunction)treemanifest_diff,
     METH_VARARGS | METH_KEYWORDS,
     "performs a diff of the given two manifests\n"},
    {"dirs",
     (PyCFunction)treemanifest_dirs,
     METH_NOARGS,
     "gets a collection of all the directories in this manifest"},
    {"filesnotin",
     (PyCFunction)treemanifest_filesnotin,
     METH_VARARGS | METH_KEYWORDS,
     "returns the set of files in m1 but not m2\n"},
    {"find",
     treemanifest_find,
     METH_VARARGS,
     "returns the node and flag for the given filepath\n"},
    {"flags",
     (PyCFunction)treemanifest_flags,
     METH_VARARGS | METH_KEYWORDS,
     "returns the flag for the given filepath\n"},
    {"get",
     (PyCFunction)treemanifest_get,
     METH_VARARGS | METH_KEYWORDS,
     "gets the node for the given filename; returns default if it doesn't "
     "exist"},
    {"hasdir",
     (PyCFunction)treemanifest_hasdir,
     METH_VARARGS,
     "returns true if the directory exists in the manifest"},
    {"iterentries",
     (PyCFunction)treemanifest_getentriesiter,
     METH_NOARGS,
     "iterate over (path, nodeid, flags) tuples in this manifest."},
    {"iterkeys",
     (PyCFunction)treemanifest_getkeysiter,
     METH_NOARGS,
     "iterate over file names in this manifest."},
    {"iteritems",
     (PyCFunction)treemanifest_iteritems,
     METH_NOARGS,
     "iterate over file names and nodes in this manifest."},
    {"keys",
     (PyCFunction)treemanifest_keys,
     METH_NOARGS,
     "list of the file names in this manifest."},
    {"listdir",
     (PyCFunction)treemanifest_listdir,
     METH_VARARGS,
     "returns a list of the files in a directory, or None if the directory "
     "doesn't exist"},
    {"matches",
     (PyCFunction)treemanifest_matches,
     METH_VARARGS,
     "returns a manifest filtered by the matcher"},
    {"set",
     treemanifest_set,
     METH_VARARGS,
     "sets the node and flag for the given filepath\n"},
    {"setflag",
     treemanifest_setflag,
     METH_VARARGS,
     "sets the flag for the given filepath\n"},
    {"text",
     (PyCFunction)treemanifest_text,
     METH_VARARGS | METH_KEYWORDS,
     "returns the text form of the manifest"},
    {"walk",
     (PyCFunction)treemanifest_walk,
     METH_VARARGS,
     "returns a iterator for walking the manifest"},
    {"walksubdirtrees",
     (PyCFunction)treemanifest_walksubdirtrees,
     METH_CLASS | METH_VARARGS | METH_KEYWORDS,
     "Returns a iterator for walking a particular subtree within a manifest."
     "`comparetrees` is a list of trees to compare against and "
     "avoid walking down any shared subtree."},
    {"walksubtrees",
     (PyCFunction)treemanifest_walksubtrees,
     METH_VARARGS | METH_KEYWORDS,
     "Returns a iterator for walking the subtree manifests."
     "`comparetrees` is a list of trees to compare against and "
     "avoid walking down any shared subtree."},
    {"finalize",
     (PyCFunction)treemanifest_finalize,
     METH_VARARGS | METH_KEYWORDS,
     "Returns an iterator that outputs each piece of the tree that is new."
     "When the iterator completes, the tree is marked as immutable."},
    {NULL, NULL}};

static PyMappingMethods treemanifest_mapping_methods = {
    0, /* mp_length */
    (binaryfunc)treemanifest_getitem, /* mp_subscript */
    (objobjargproc)treemanifest_setitem, /* mp_ass_subscript */
};

static PySequenceMethods treemanifest_sequence_methods = {
    0, /* sq_length */
    0, /* sq_concat */
    0, /* sq_repeat */
    0, /* sq_item */
    0, /* sq_slice */
    0, /* sq_ass_item */
    0, /* sq_ass_slice */
    (objobjproc)treemanifest_contains, /* sq_contains */
    0, /* sq_inplace_concat */
    0, /* sq_inplace_repeat */
};

static PyNumberMethods treemanifest_number_methods = {
    0, /* binaryfunc nb_add; */
    0, /* binaryfunc nb_subtract; */
    0, /* binaryfunc nb_multiply; */
    0, /* binaryfunc nb_divide; */
    0, /* binaryfunc nb_remainder; */
    0, /* binaryfunc nb_divmod; */
    0, /* ternaryfunc nb_power; */
    0, /* unaryfunc nb_negative; */
    0, /* unaryfunc nb_positive; */
    0, /* unaryfunc nb_absolute; */
    (inquiry)treemanifest_nonzero, /* inquiry nb_nonzero; */
};

static PyTypeObject treemanifestType = {
    PyObject_HEAD_INIT(NULL) 0, /* ob_size */
    "cstore.treemanifest", /* tp_name */
    sizeof(py_treemanifest), /* tp_basicsize */
    0, /* tp_itemsize */
    (destructor)treemanifest_dealloc, /* tp_dealloc */
    0, /* tp_print */
    0, /* tp_getattr */
    0, /* tp_setattr */
    0, /* tp_compare */
    0, /* tp_repr */
    &treemanifest_number_methods, /* tp_as_number */
    &treemanifest_sequence_methods, /* tp_as_sequence - length/contains */
    &treemanifest_mapping_methods, /* tp_as_mapping - getitem/setitem */
    0, /* tp_hash */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */
    Py_TPFLAGS_DEFAULT, /* tp_flags */
    "TODO", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    (getiterfunc)treemanifest_getkeysiter, /* tp_iter */
    0, /* tp_iternext */
    treemanifest_methods, /* tp_methods */
    0, /* tp_members */
    0, /* tp_getset */
    0, /* tp_base */
    0, /* tp_dict */
    0, /* tp_descr_get */
    0, /* tp_descr_set */
    0, /* tp_dictoffset */
    (initproc)treemanifest_init, /* tp_init */
    0, /* tp_alloc */
};

#endif /* FBHGEXT_CSTORE_PY_TREEMANIFEST_H */
