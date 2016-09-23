// py-treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <string>

#include "convert.h"
#include "manifest.h"
#include "pythonutil.h"
#include "treemanifest.h"

#define FILENAME_BUFFER_SIZE 16348
#define FLAG_SIZE 1

struct py_treemanifest {
  PyObject_HEAD;

  treemanifest tm;
};

/**
 * The python iteration object for iterating over a tree.  This is separate from
 * the fileiter above because it lets us just call the constructor on
 * fileiter, which will automatically populate all the members of fileiter.
 */
struct py_fileiter {
  PyObject_HEAD;

  fileiter iter;

  bool includenodeflag;

  // A reference to the tree is kept, so it is not freed while we're iterating
  // over it.
  const py_treemanifest *treemf;
};

static void fileiter_dealloc(py_fileiter *self);
static PyObject* fileiter_iterentriesnext(py_fileiter *self);
static PyTypeObject fileiterType = {
  PyObject_HEAD_INIT(NULL)
  0,                               /*ob_size */
  "treemanifest.keyiter",          /*tp_name */
  sizeof(py_fileiter),                /*tp_basicsize */
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
  (iternextfunc)fileiter_iterentriesnext, /* tp_iternext: next() method */
};

static py_fileiter *createfileiter(py_treemanifest *pytm,
                                   bool includenodeflag,
                                   bool sorted,
                                   PythonObj matcher) {
  py_fileiter *i = PyObject_New(py_fileiter, &fileiterType);
  if (i) {
    try {
      i->treemf = pytm;
      Py_INCREF(pytm);
      i->includenodeflag = includenodeflag;

      // The provided py_fileiter struct hasn't initialized our fileiter member, so
      // we do it manually.
      new (&i->iter) fileiter(pytm->tm, sorted);
      i->iter.matcher = matcher;
      return i;
    } catch (const pyexception &ex) {
      Py_DECREF(i);
      return NULL;
    } catch (const std::exception &ex) {
      Py_DECREF(i);
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      return NULL;
    }
  } else {
    return NULL;
  }
}

static py_fileiter *createfileiter(py_treemanifest *pytm,
                                   bool includenodeflag) {
  return createfileiter(
      pytm,
      includenodeflag,
      true,                       // we care about sort order.
      PythonObj());
}

// ==== treemanifest functions ====

/**
 * Implementation of treemanifest.__iter__
 * Returns a PyObject iterator instance.
 */
static PyObject *treemanifest_getkeysiter(py_treemanifest *self) {
  return (PyObject*)createfileiter(self, false);
}

static PyObject *treemanifest_diff(
    PyObject *o, PyObject *args, PyObject *kwargs) {
  py_treemanifest *self = (py_treemanifest*)o;
  PyObject *otherObj;
  PyObject *cleanObj;
  static char const *kwlist[] = {"m2", "clean", NULL};

  if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|O", (char**)kwlist, &otherObj, &cleanObj)) {
    return NULL;
  }

  py_treemanifest *other = (py_treemanifest*)otherObj;

  PythonObj results = PyDict_New();

  ManifestFetcher fetcher = self->tm.fetcher;

  std::string path;
  try {
    path.reserve(1024);

    treemanifest_diffrecurse(
        self->tm.getRootManifest(),
        other->tm.getRootManifest(),
        path, results, fetcher);
  } catch (const pyexception &ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return results.returnval();
}

/**
 * Implementation of treemanifest.find()
 * Takes a filename and returns a tuple of the binary hash and flag,
 * or (None, None) if it doesn't exist.
 */
static PyObject *treemanifest_find(PyObject *o, PyObject *args) {
  py_treemanifest *self = (py_treemanifest*)o;
  char *filename;
  Py_ssize_t filenamelen;

  if (!PyArg_ParseTuple(args, "s#", &filename, &filenamelen)) {
    return NULL;
  }

  std::string resultnode;
  const char *resultflag;
  try {
    // Grab the root node's data

    self->tm.get(
        std::string(filename, (size_t) filenamelen),
        &resultnode, &resultflag);
  } catch (const pyexception &ex) {
    return NULL;
  }

  if (resultnode.empty()) {
    if (PyErr_Occurred()) {
      return NULL;
    }
    return Py_BuildValue("s#s#", NULL, (Py_ssize_t)0, NULL, (Py_ssize_t)0);
  } else {
    Py_ssize_t flaglen;
    if (resultflag == NULL) {
      flaglen = 0;
      resultflag = MAGIC_EMPTY_STRING;
    } else {
      flaglen = 1;
    }
    return Py_BuildValue("s#s#",
        resultnode.c_str(), (Py_ssize_t)resultnode.length(),
        resultflag, flaglen);
  }
}

/**
 * Implementation of treemanifest.set()
 * Takes a binary hash and flag and sets it for a given filename.
 */
static PyObject *treemanifest_set(PyObject *o, PyObject *args) {
  py_treemanifest *self = (py_treemanifest*)o;
  char *filename;
  Py_ssize_t filenamelen;
  char *hash;
  Py_ssize_t hashlen;
  char *flagstr;
  Py_ssize_t flagstrlen;
  const char *flag;

  if (!PyArg_ParseTuple(args, "s#z#z#",
      &filename, &filenamelen,
      &hash, &hashlen,
      &flagstr, &flagstrlen)) {
    return NULL;
  }

  // verify that the lengths of the fields are sane.
  if (hash == NULL && flagstr == NULL) {
    // this is a remove operation!!
    self->tm.remove(std::string(filename, (size_t) filenamelen));
    Py_RETURN_NONE;
  } else if (hashlen != BIN_NODE_SIZE) {
    PyErr_Format(PyExc_ValueError,
        "hash length must be %d bytes long", BIN_NODE_SIZE);
    return NULL;
  } else if (flagstrlen > 1) {
    PyErr_Format(PyExc_ValueError,
        "flags must either be 0 or 1 byte long");
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

    SetResult result = self->tm.set(
        std::string(filename, (size_t) filenamelen),
        hashstr,
        flag);

    if (result == SET_OK) {
      Py_RETURN_NONE;
    } else {
      PyErr_Format(PyExc_TypeError, "unexpected stuff happened");
      return NULL;
    }
  } catch (const pyexception &ex) {
    return NULL;
  }
}

/*
 * Deallocates the contents of the treemanifest
 */
static void treemanifest_dealloc(py_treemanifest *self) {
  self->tm.~treemanifest();
  PyObject_Del(self);
}

/*
 * Initializes the contents of a treemanifest
 */
static int treemanifest_init(py_treemanifest *self, PyObject *args) {
  PyObject *store;
  char *node = NULL;
  Py_ssize_t nodelen;

  if (!PyArg_ParseTuple(args, "O|s#", &store, &node, &nodelen)) {
    return -1;
  }

  Py_INCREF(store);

  // We have to manually call the member constructor, since the provided 'self'
  // is just zerod out memory.
  try {
    if (node != NULL) {
      new(&self->tm) treemanifest(
          PythonObj(store), std::string(node, (size_t) nodelen));
    } else {
      new(&self->tm) treemanifest(PythonObj(store));
    }
  } catch (const std::exception &ex) {
    Py_DECREF(store);
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
static void fileiter_dealloc(py_fileiter *self) {
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
static bool fileiter_popfinished(fileiter *iter) {
  stackframe *frame = &iter->frames.back();

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
static bool fileiter_next(fileiter &iter, char *path, size_t pathcapacity,
                          char *node, char *flag) {
  // Iterate over the current directory contents
  while (true) {
    // Pop off any directories that we're done processing
    if (!fileiter_popfinished(&iter)) {
      // No more directories means we've reached the end of the root
      return false;
    }

    stackframe &frame = iter.frames.back();

    ManifestEntry *entry;
    entry = frame.next();

    // If a directory, push it and loop again
    if (entry->isdirectory()) {
      iter.path.append(entry->filename, entry->filenamelen);
      iter.path.append(1, '/');

      Manifest *submanifest = entry->get_manifest(iter.fetcher,
          iter.path.c_str(), iter.path.size());

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

      if ((PyObject*)iter.matcher) {
        PythonObj matchArgs = Py_BuildValue("(s#)", path, pathlen);
        PythonObj matched = iter.matcher.call(matchArgs);
        if (!PyObject_IsTrue(matched)) {
          continue;
        }
      }

      std::string binnode = binfromhex(entry->node);
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
static PyObject *fileiter_iterentriesnext(py_fileiter *self) {
  fileiter &iter = self->iter;

  try {
    char path[FILENAME_BUFFER_SIZE];
    char node[BIN_NODE_SIZE];
    char flag[FLAG_SIZE];
    if (fileiter_next(iter, path, FILENAME_BUFFER_SIZE, node, flag)) {
      if (self->includenodeflag) {
        size_t flaglen = 0;
        if (flag[0] != '\0') {
          flaglen = 1;
        }
        return Py_BuildValue("(s#s#s#)", path, strlen(path),
                                         node, BIN_NODE_SIZE,
                                         flag, flaglen);
      } else {
        return PyString_FromStringAndSize(path, strlen(path));
      }
    }
    return NULL;
  } catch (const pyexception &ex) {
    return NULL;
  }
}

/**
 * Implements treemanifest.__getitem__(path)
 * Returns the node of the given file.
 */
static PyObject *treemanifest_getitem(py_treemanifest *self, PyObject *key) {
  char *filename;
  Py_ssize_t filenamelen;
  PyString_AsStringAndSize(key, &filename, &filenamelen);

  std::string resultnode;
  const char *resultflag;
  try {
    self->tm.get(
        std::string(filename, (size_t) filenamelen),
        &resultnode, &resultflag);
  } catch (const pyexception &ex) {
    return NULL;
  }

  if (resultnode.empty()) {
    if (PyErr_Occurred()) {
      return NULL;
    }

    PyErr_Format(PyExc_KeyError, "file '%s' not found", filename);
    return NULL;
  } else {
    return Py_BuildValue("s#", resultnode.c_str(), (Py_ssize_t)resultnode.length());
  }
}

/**
 * Implements treemanifest.flags(path)
 * Returns the flag of the given file.
 */
static PyObject *treemanifest_flags(py_treemanifest *self, PyObject *args, PyObject *kwargs) {
  char *filename;
  Py_ssize_t filenamelen;
  char *defaultval= NULL;
  Py_ssize_t defaultvallen;
  static char const *kwlist[] = {"key", "default", NULL};

  if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|s#", (char**)kwlist,
                                   &filename, &filenamelen,
                                   &defaultval, &defaultvallen)) {
    return NULL;
  }

  std::string resultnode;
  const char *resultflag;
  try {
    self->tm.get(
        std::string(filename, (size_t) filenamelen),
        &resultnode, &resultflag);
  } catch (const pyexception &ex) {
    return NULL;
  }

  if (resultnode.empty()) {
    if (PyErr_Occurred()) {
      return NULL;
    }

    PyErr_Format(PyExc_KeyError, "file '%s' not found", filename);
    return NULL;
  }

  if (resultflag == NULL) {
    if (defaultval) {
      return PyString_FromStringAndSize(defaultval, defaultvallen);
    } else {
      return PyString_FromStringAndSize(MAGIC_EMPTY_STRING, (Py_ssize_t)0);
    }
  } else {
    return PyString_FromStringAndSize(resultflag, (Py_ssize_t)1);
  }
}

static PyObject *treemanifest_copy(py_treemanifest *self) {
  PythonObj module = PyImport_ImportModule("ctreemanifest");
  PythonObj treetype = module.getattr("treemanifest");
  py_treemanifest *copy = PyObject_New(py_treemanifest, (PyTypeObject*)(PyObject*)treetype);
  PythonObj copyObj((PyObject*)copy);

  new(&copy->tm) treemanifest(self->tm);

  return copyObj.returnval();
}

static PyObject *treemanifest_matches(py_treemanifest *self, PyObject *args) {
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
    PythonObj manifestmod = PyImport_ImportModule("mercurial.manifest");
    PythonObj manifestdict = manifestmod.getattr("manifestdict");
    PythonObj result = manifestdict.call(emptyargs);

    fileiter iter = fileiter(self->tm, false);
    iter.matcher = matcher;

    char path[2048];
    char node[HEX_NODE_SIZE];
    char flag[1];
    while (fileiter_next(iter, path, 2048, node, flag)) {
      size_t pathlen = strlen(path);
      std::string binnode = binfromhex(node);

      // Call manifestdict.__setitem__
      PythonObj setArgs = Py_BuildValue(
          "s#s#",
          path, pathlen, binnode.c_str(), BIN_NODE_SIZE);
      result.callmethod("__setitem__", setArgs);

      Py_ssize_t flaglen = *flag != '\0' ? 1 : 0;
      PythonObj flagArgs = Py_BuildValue("s#s#", path, pathlen, flag, flaglen);
      result.callmethod("setflag", flagArgs);
    }

    return result.returnval();
  } catch (const pyexception &ex) {
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return NULL;
}

static PyObject *treemanifest_filesnotin(py_treemanifest *self, PyObject *args) {
  py_treemanifest* other;

  if (!PyArg_ParseTuple(args, "O", &other)) {
    return NULL;
  }

  PythonObj diffresults = PyDict_New();

  ManifestFetcher fetcher = self->tm.fetcher;

  std::string path;
  try {
    path.reserve(1024);
    treemanifest_diffrecurse(
        self->tm.getRootManifest(),
        other->tm.getRootManifest(),
        path, diffresults, fetcher);
  } catch (const pyexception &ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  PythonObj result = PySet_New(NULL);

  // All of the PyObjects below are borrowed references, so no ref counting is
  // required.
  Py_ssize_t iterpos = 0;
  PyObject *pathkey;
  PyObject *diffentry;
  while (PyDict_Next(diffresults, &iterpos, &pathkey, &diffentry)) {
    // Each value is a `((m1node, m1flag), (m2node, m2flag))` tuple.
    // If m2node is None, then this file doesn't exist in m2.
    PyObject *targetvalue = PyTuple_GetItem(diffentry, 1);
    if (!targetvalue) {
      return NULL;
    }
    PyObject *targetnode = PyTuple_GetItem(targetvalue, 0);
    if (!targetnode) {
      return NULL;
    }
    if (targetnode == Py_None) {
      PySet_Add(result, pathkey);
    }
  }

  return result.returnval();
}

static int treemanifest_contains(py_treemanifest *self, PyObject *key) {
  char *filename;
  Py_ssize_t filenamelen;
  PyString_AsStringAndSize(key, &filename, &filenamelen);

  std::string resultnode;
  const char *resultflag;
  try {
    self->tm.get(
        std::string(filename, (size_t) filenamelen),
        &resultnode, &resultflag);
    if (resultnode.size() == 0) {
      return 0;
    } else {
      return 1;
    }
  } catch (const pyexception &ex) {
    return -1;
  }
}

static PyObject *treemanifest_getentriesiter(py_treemanifest *self) {
  return (PyObject*)createfileiter(self, true);
}

static PyObject *treemanifest_walk(py_treemanifest *self, PyObject *args) {
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
      false,                  // walk does not care about sort order.
      matcher);
}

void writestore(Manifest *mainManifest, const std::vector<char*> &cmpNodes,
                const std::vector<Manifest*> &cmpManifests,
                PythonObj &pack, const ManifestFetcher &fetcher) {
  NewTreeIterator iterator(mainManifest, cmpNodes, cmpManifests, fetcher);

  std::string *path = NULL;
  Manifest *result = NULL;
  std::string *node = NULL;
  std::string raw;
  while (iterator.next(&path, &result, &node)) {
    // TODO: find an appropriate delta base and compute the delta
    result->serialize(raw);
    PythonObj args = Py_BuildValue("(s#s#s#s#)",
                                   path->c_str(), (Py_ssize_t)path->size(),
                                   node->c_str(), (Py_ssize_t)BIN_NODE_SIZE,
                                   NULLID, (Py_ssize_t)BIN_NODE_SIZE,
                                   raw.c_str(), (Py_ssize_t)raw.size());

    pack.callmethod("add", args);
  }
}

static PyObject *treemanifest_write(py_treemanifest *self, PyObject *args) {
  PyObject* packObj;
  py_treemanifest* p1tree = NULL;

  if (!PyArg_ParseTuple(args, "O|O", &packObj, &p1tree)) {
    return NULL;
  }

  // ParseTuple doesn't increment the ref, but the PythonObj will decrement on
  // destruct, so let's increment now.
  Py_INCREF(packObj);
  PythonObj pack = packObj;

  try {
    std::vector<char*> cmpNodes;
    std::vector<Manifest*> cmpManifests;
    if (p1tree) {
      assert(p1tree->tm.root.node);
      cmpNodes.push_back(p1tree->tm.root.node);
      cmpManifests.push_back(p1tree->tm.getRootManifest());
    }

    writestore(self->tm.getRootManifest(), cmpNodes, cmpManifests, pack, self->tm.fetcher);

    char tempnode[20];
    self->tm.getRootManifest()->computeNode(p1tree ? binfromhex(p1tree->tm.root.node).c_str() : NULLID, NULLID, tempnode);
    std::string hexnode;
    hexfrombin(tempnode, hexnode);
    self->tm.root.update(hexnode.c_str(), MANIFEST_DIRECTORY_FLAGPTR);

    return PyString_FromStringAndSize(tempnode, BIN_NODE_SIZE);
  } catch (const pyexception &ex) {
    return NULL;
  }
}

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
  {"copy", (PyCFunction)treemanifest_copy, METH_NOARGS, "copies the treemanifest"},
  {"diff", (PyCFunction)treemanifest_diff, METH_VARARGS|METH_KEYWORDS, "performs a diff of the given two manifests\n"},
  {"filesnotin", (PyCFunction)treemanifest_filesnotin, METH_VARARGS, "returns the set of files in m1 but not m2\n"},
  {"find", treemanifest_find, METH_VARARGS, "returns the node and flag for the given filepath\n"},
  {"flags", (PyCFunction)treemanifest_flags, METH_VARARGS|METH_KEYWORDS,
    "returns the flag for the given filepath\n"},
  {"iterentries", (PyCFunction)treemanifest_getentriesiter, METH_NOARGS,
   "iterate over (path, nodeid, flags) tuples in this manifest."},
  {"iterkeys", (PyCFunction)treemanifest_getkeysiter, METH_NOARGS,
   "iterate over file names in this manifest."},
  {"matches", (PyCFunction)treemanifest_matches, METH_VARARGS,
    "returns a manifest filtered by the matcher"},
  {"set", treemanifest_set, METH_VARARGS,
      "sets the node and flag for the given filepath\n"},
  {"walk", (PyCFunction)treemanifest_walk, METH_VARARGS,
    "returns a iterator for walking the manifest"},
  {"write", (PyCFunction)treemanifest_write, METH_VARARGS,
    "writes any pending tree changes to the given store"},
  {NULL, NULL}
};

static PyMappingMethods treemanifest_mapping_methods = {
  0,                                   /* mp_length */
  (binaryfunc)treemanifest_getitem,    /* mp_subscript */
  0,                                   /* mp_ass_subscript */
};

static PySequenceMethods treemanifest_sequence_methods = {
	0,                                 /* sq_length */
	0,                                 /* sq_concat */
	0,                                 /* sq_repeat */
	0,                                 /* sq_item */
	0,                                 /* sq_slice */
	0,                                 /* sq_ass_item */
	0,                                 /* sq_ass_slice */
	(objobjproc)treemanifest_contains, /* sq_contains */
	0,                                 /* sq_inplace_concat */
	0,                                 /* sq_inplace_repeat */
};

static PyTypeObject treemanifestType = {
  PyObject_HEAD_INIT(NULL)
  0,                                                /* ob_size */
  "ctreemanifest.treemanifest",                     /* tp_name */
  sizeof(py_treemanifest),                          /* tp_basicsize */
  0,                                                /* tp_itemsize */
  (destructor)treemanifest_dealloc,                 /* tp_dealloc */
  0,                                                /* tp_print */
  0,                                                /* tp_getattr */
  0,                                                /* tp_setattr */
  0,                                                /* tp_compare */
  0,                                                /* tp_repr */
  0,                                                /* tp_as_number */
  &treemanifest_sequence_methods,                   /* tp_as_sequence - length/contains */
  &treemanifest_mapping_methods,                    /* tp_as_mapping - getitem/setitem */
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
  (getiterfunc)treemanifest_getkeysiter,            /* tp_iter */
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
