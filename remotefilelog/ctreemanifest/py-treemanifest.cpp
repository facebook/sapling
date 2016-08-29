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

  // A reference to the tree is kept, so it is not freed while we're iterating
  // over it.
  const py_treemanifest *treemf;

  py_fileiter(ManifestFetcher fetcher) :
    iter(fetcher) {
  }
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

// ==== treemanifest functions ====

/**
 * Implementation of treemanifest.__iter__
 * Returns a PyObject iterator instance.
 */
static PyObject *treemanifest_getkeysiter(py_treemanifest *self) {
  py_fileiter *i = PyObject_New(py_fileiter, &fileiterType);
  if (i) {
    try {
      i->treemf = self;
      Py_INCREF(i->treemf);

      ManifestFetcher fetcher(self->tm.store);
      // The provided py_fileiter struct hasn't initialized our fileiter member, so
      // we do it manually.
      new (&i->iter) fileiter(fetcher);

      // Grab the root node's data and prep the iterator
      if (self->tm.rootManifest == NULL) {
        self->tm.rootManifest = fetcher.get(NULL, 0, self->tm.rootNode);
      }

      i->iter.frames.push_back(stackframe(self->tm.rootManifest));

      i->iter.path.reserve(1024);
    } catch (const pyexception &ex) {
      Py_DECREF(i);
      return NULL;
    } catch (const std::exception &ex) {
      Py_DECREF(i);
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      Py_DECREF(i);
      return NULL;
    }
  } else {
    return NULL;
  }

  return (PyObject *)i;
}

/**
 * Constructs a result python tuple of the given diff data.
 */
static PythonObj treemanifest_diffentry(const std::string *anode, const char *aflag,
                                        const std::string *bnode, const char *bflag) {
  const char *astr = anode != NULL ? anode->c_str() : NULL;
  Py_ssize_t alen = anode != NULL ? anode->length() : 0;
  const char *bstr = bnode != NULL ? bnode->c_str() : NULL;
  Py_ssize_t blen = bnode != NULL ? bnode->length() : 0;
  PythonObj result = Py_BuildValue("((s#s#)(s#s#))", astr, alen, aflag, Py_ssize_t(aflag ? 1 : 0),
                                                     bstr, blen, bflag, Py_ssize_t(bflag ? 1 : 0));
  return result;
}

/**
 * Simple class for representing a single diff between two files in the
 * manifest.
 */
class DiffEntry {
  private:
    const std::string *selfnode;
    const std::string *othernode;
    const char *selfflag;
    const char *otherflag;
  public:
    DiffEntry(const std::string *selfnode, const char *selfflag,
              const std::string *othernode, const char *otherflag) {
      this->selfnode = selfnode;
      this->othernode = othernode;
      this->selfflag = selfflag;
      this->otherflag = otherflag;
    }

    void addtodiff(const PythonObj &diff, const std::string &path) {
      PythonObj entry = treemanifest_diffentry(this->selfnode, this->selfflag,
                                               this->othernode, this->otherflag);
      PythonObj pathObj = PyString_FromStringAndSize(path.c_str(), path.length());

      PyDict_SetItem(diff, pathObj, entry);
    }
};

/**
 * Helper function that performs the actual recursion on the tree entries.
 */
static void treemanifest_diffrecurse(
    Manifest *selfmf,
    Manifest *othermf,
    std::string &path,
    const PythonObj &diff,
    const ManifestFetcher &fetcher) {
  ManifestIterator selfiter;
  ManifestIterator otheriter;

  if (selfmf != NULL) {
    selfiter = selfmf->getIterator();
  }
  if (othermf != NULL) {
    otheriter = othermf->getIterator();
  }

  // Iterate through both directory contents
  while (!selfiter.isfinished() || !otheriter.isfinished()) {
    int cmp = 0;

    ManifestEntry *selfentry = NULL;
    std::string selfbinnode;
    if (!selfiter.isfinished()) {
      cmp--;
      selfentry = selfiter.currentvalue();
      selfbinnode = binfromhex(selfentry->node);
    }

    ManifestEntry *otherentry = NULL;
    std::string otherbinnode;
    if (!otheriter.isfinished()) {
      cmp++;
      otherentry = otheriter.currentvalue();
      otherbinnode = binfromhex(otherentry->node);
    }

    // If both sides are present, cmp == 0, so do a filename comparison
    if (cmp == 0) {
      cmp = strcmp(selfentry->filename, otherentry->filename);
    }

    int originalpathsize = path.size();
    if (cmp < 0) {
      // selfentry should be processed first and only exists in self
      selfentry->appendtopath(path);
      if (selfentry->isdirectory()) {
        Manifest *selfchildmanifest = selfentry->get_manifest(
            fetcher, path.c_str(), path.size());
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);
      } else {
        DiffEntry entry(&selfbinnode, selfentry->flag, NULL, NULL);
        entry.addtodiff(diff, path);
      }
      selfiter.next();
    } else if (cmp > 0) {
      // otherentry should be processed first and only exists in other
      otherentry->appendtopath(path);
      if (otherentry->isdirectory()) {
        Manifest *otherchildmanifest = otherentry->get_manifest(
            fetcher, path.c_str(), path.size());
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);
      } else {
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry->flag);
        entry.addtodiff(diff, path);
      }
      otheriter.next();
    } else {
      // Filenames match - now compare directory vs file
      if (selfentry->isdirectory() && otherentry->isdirectory()) {
        // Both are directories - recurse
        selfentry->appendtopath(path);

        if (selfbinnode != otherbinnode) {
          Manifest *selfchildmanifest = fetcher.get(
              path.c_str(), path.size(),
              selfbinnode);
          Manifest *otherchildmanifest = fetcher.get(
              path.c_str(), path.size(),
              otherbinnode);

          treemanifest_diffrecurse(
              selfchildmanifest,
              otherchildmanifest,
              path,
              diff,
              fetcher);
        }
        selfiter.next();
        otheriter.next();
      } else if (selfentry->isdirectory() && !otherentry->isdirectory()) {
        // self is directory, other is not - process other then self
        otherentry->appendtopath(path);
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry->flag);
        entry.addtodiff(diff, path);

        path.append(1, '/');
        Manifest *selfchildmanifest = fetcher.get(
            path.c_str(), path.size(),
            selfbinnode);
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);

        selfiter.next();
        otheriter.next();
      } else if (!selfentry->isdirectory() && otherentry->isdirectory()) {
        // self is not directory, other is - process self then other
        selfentry->appendtopath(path);
        DiffEntry entry(&selfbinnode, selfentry->flag, NULL, NULL);
        entry.addtodiff(diff, path);

        path.append(1, '/');
        Manifest *otherchildmanifest = fetcher.get(
            path.c_str(), path.size(),
            otherbinnode
        );
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);

        selfiter.next();
        otheriter.next();
      } else {
        // both are files
        bool flagsdiffer = (
          (selfentry->flag && otherentry->flag && *selfentry->flag != *otherentry->flag) ||
          ((bool)selfentry->flag != (bool)selfentry->flag)
        );

        if (selfbinnode != otherbinnode || flagsdiffer) {
          selfentry->appendtopath(path);
          DiffEntry entry(&selfbinnode, selfentry->flag, &otherbinnode, otherentry->flag);
          entry.addtodiff(diff, path);
        }

        selfiter.next();
        otheriter.next();
      }
    }
    path.erase(originalpathsize);
  }
}

static PyObject *treemanifest_diff(PyObject *o, PyObject *args) {
  py_treemanifest *self = (py_treemanifest*)o;
  PyObject *otherObj;

  if (!PyArg_ParseTuple(args, "O", &otherObj)) {
    return NULL;
  }

  py_treemanifest *other = (py_treemanifest*)otherObj;

  PythonObj results = PyDict_New();

  ManifestFetcher fetcher(self->tm.store);

  std::string path;
  try {
    path.reserve(1024);

    // Grab the root node's data
    if (self->tm.rootManifest == NULL) {
      self->tm.rootManifest = fetcher.get(NULL, 0, self->tm.rootNode);
      // TODO: error handling
    }

    // Grab the root node's data
    if (other->tm.rootManifest == NULL) {
      other->tm.rootManifest = fetcher.get(NULL, 0, other->tm.rootNode);
      // TODO: error handling
    }

    treemanifest_diffrecurse(
        self->tm.rootManifest,
        other->tm.rootManifest,
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

  ManifestFetcher fetcher(self->tm.store);

  std::string resultnode;
  char resultflag;
  try {
    _treemanifest_find(
        std::string(filename, filenamelen),
        self->tm.rootNode,
        &self->tm.rootManifest,
        fetcher,
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
    int flaglen = 0;
    if (resultflag != '\0') {
      flaglen = 1;
    }
    return Py_BuildValue("s#s#", resultnode.c_str(), (Py_ssize_t)resultnode.length(), &resultflag, (Py_ssize_t)flaglen);
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
  char *node;
  Py_ssize_t nodelen;

  if (!PyArg_ParseTuple(args, "Os#", &store, &node, &nodelen)) {
    return -1;
  }

  Py_INCREF(store);

  // We have to manually call the member constructor, since the provided 'self'
  // is just zerod out memory.
  try {
    new (&self->tm) treemanifest(PythonObj(store), std::string(node, nodelen));
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
  while (frame->iterator.isfinished()) {
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
      iter->path.erase(0);
    }
  }

  return true;
}

/**
 * Returns the next object in the iteration.
 */
static PyObject *fileiter_iterentriesnext(py_fileiter *self) {
  fileiter &iter = self->iter;

  try {
    // Iterate over the current directory contents
    while (true) {
      // Pop off any directories that we're done processing
      if (!fileiter_popfinished(&iter)) {
        // No more directories means we've reached the end of the root
        return NULL;
      }

      stackframe &frame = iter.frames.back();
      ManifestIterator &iterator = frame.iterator;

      ManifestEntry* entry;
      entry = iterator.next();

      // If a directory, push it and loop again
      if (entry->isdirectory()) {
        iter.path.append(entry->filename, entry->filenamelen);
        iter.path.append(1, '/');

        Manifest *submanifest = entry->get_manifest(iter.fetcher,
            iter.path.c_str(), iter.path.size());

        // TODO: memory cleanup here is probably broken.
        iter.frames.push_back(stackframe(submanifest));

      } else {
        // If a file, yield it
        int oldpathsize = iter.path.size();
        iter.path.append(entry->filename, entry->filenamelen);
        PyObject* result = PyString_FromStringAndSize(iter.path.c_str(), iter.path.length());
        if (!result) {
          PyErr_NoMemory();
          return NULL;
        }

        iter.path.erase(oldpathsize);
        return result;
      }
    }
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

  ManifestFetcher fetcher(self->tm.store);

  std::string resultnode;
  char resultflag;
  try {
    _treemanifest_find(
        std::string(filename, filenamelen),
        self->tm.rootNode,
        &self->tm.rootManifest,
        fetcher,
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

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
  {"diff", treemanifest_diff, METH_VARARGS, "performs a diff of the given two manifests\n"},
  {"find", treemanifest_find, METH_VARARGS, "returns the node and flag for the given filepath\n"},
  {NULL, NULL}
};

static PyMappingMethods treemanifest_mapping_methods = {
  0,                                   /* mp_length */
  (binaryfunc)treemanifest_getitem,    /* mp_subscript */
  0,                                   /* mp_ass_subscript */
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
  0,                                                /* tp_as_sequence - length/contains */
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
