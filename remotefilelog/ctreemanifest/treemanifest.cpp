// treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// The PY_SSIZE_T_CLEAN define must be defined before the Python.h include,
// as per the documentation.
#define PY_SSIZE_T_CLEAN
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

typedef struct {
  char *filename;
  size_t filenamelen;
  char *node;
  char *flag;
  char *nextentrystart;

} manifestentry;

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
static PyObject* fileiter_iterentriesnext(fileiter *self);
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
  (iternextfunc)fileiter_iterentriesnext, /* tp_iternext: next() method */
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

  arglist = Py_BuildValue("s#s#", dir.c_str(), (Py_ssize_t)dir.size(),
                                  node.c_str(), (Py_ssize_t)node.size());
  if (!arglist) {
    return NULL;
  }

  result = PyEval_CallObject(get, arglist);
  Py_DECREF(arglist);

  if (!result) {
    PyErr_Format(PyExc_RuntimeError, "unable to find tree '%s:...'", dir.c_str());
  }

  return result;
}

/* Given the start of a file/dir entry in a manifest, parsentry returns a
 * manifestentry structure with the parsed data.
 * */
static manifestentry parseentry(char *entrystart) {
  manifestentry result;
  // Each entry is of the format:
  //
  //   <filename>\0<40-byte hash><optional 1 byte flag>\n
  //
  // Where flags can be 't' to represent a sub directory
  result.filename = entrystart;
  char *nulldelimiter = strchr(entrystart, '\0');
  result.filenamelen = nulldelimiter - entrystart;

  result.node = nulldelimiter + 1;

  result.flag = nulldelimiter + 41;
  if (*result.flag != '\n') {
    result.nextentrystart = result.flag + 2;
  } else {
    // No flag
    result.nextentrystart = result.flag + 1;
    result.flag = NULL;
  }

  return result;
}

class ManifestIterator {
  private:
    char *raw;
    char *entrystart;
    int length;
  public:
    ManifestIterator(char *raw, size_t length) {
      this->raw = raw;
      this->entrystart = raw;
      this->length = length;
    }

    bool next(manifestentry *entry) {
      if (this->entrystart - this->raw >= this->length) {
        return false;
      }

      *entry = parseentry(this->entrystart);
      this->entrystart = entry->nextentrystart;
      return true;
    }
};

// ==== treemanifest functions ====

/* Implementation of treemanifest.__iter__
 * Returns a PyObject iterator instance.
 * */
static PyObject *treemanifest_getkeysiter(treemanifest *self) {
  fileiter *i = PyObject_New(fileiter, &fileiterType);
  if (i) {
    try {
      // The provided fileiter struct hasn't initialized our stackiter member, so
      // we do it manually.
      new (&i->iter) stackiter();

      // Keep a copy of the store's get function for accessing contents
      i->iter.get = PyObject_GetAttrString(self->store, "get");

      // Grab the root node's data and prep the iterator
      PyObject *rawobj = getdata(i->iter.get, "", self->node);
      if (!rawobj) {
        Py_DECREF(i);
        return NULL;
      }

      char *raw;
      Py_ssize_t rawsize;
      PyString_AsStringAndSize(rawobj, &raw, &rawsize);

      i->iter.data.push_back(rawobj);
      i->iter.location.push_back(raw);
      i->iter.path.reserve(1024);
    } catch (const std::exception &ex) {
      PyErr_SetString(PyExc_RuntimeError, ex.what());
      Py_DECREF(i);
      return NULL;
    }
  } else {
    return NULL;
  }

  return (PyObject *)i;
}

class PathIterator {
  private:
    std::string path;
    size_t position;
  public:
    PathIterator(std::string path) {
      this->path = path;
      this->position = 0;
    }

    bool next(char const ** word, size_t *wordlen) {
      if (this->isfinished()) {
        return false;
      }

      *word = this->path.c_str() + this->position;
      size_t slashoffset = this->path.find('/', this->position);
      if (slashoffset == std::string::npos) {
        *wordlen = this->path.length() - this->position;
      } else {
        *wordlen = slashoffset - this->position;
      }

      this->position += *wordlen + 1;

      return true;
    }

    bool isfinished() {
      return this->position >= this->path.length();
    }
};

/* Helper find function that perform the actual recursion on the tree entries.
 * */
static void _treemanifest_find(const std::string &filename, const std::string &node,
        PyObject *get, std::string *resultnode, char *resultflag) {
  std::string curnode = node;
  const char *filenamecstr = filename.c_str();

  // Loop over the parts of the query filename
  PathIterator pathiter(filename);
  const char *word;
  size_t wordlen;
  while (pathiter.next(&word, &wordlen)) {
    // Get the fullpath of the current directory/file we're searching in
    std::string curpath = filename.substr(0, word - filenamecstr);

    // Obtain the raw data for this directory
    PyObject* rawobj = getdata(get, curpath, curnode);
    if (!rawobj) {
        return;
    }
    char* raw;
    Py_ssize_t rawsize;
    PyString_AsStringAndSize(rawobj, &raw, &rawsize);

    ManifestIterator mfiterator(raw, rawsize);
    manifestentry entry;
    bool recurse = false;

    // Loop over the contents of the current directory looking for the
    // next directory/file.
    while (mfiterator.next(&entry)) {
      // If the current entry matches the query file/directory, either recurse,
      // return, or abort.
      if (wordlen == entry.filenamelen && strncmp(word, entry.filename, wordlen) == 0) {
        // If this is the last entry in the query path, either return or abort
        if (pathiter.isfinished()) {
          // If it's a file, it's our result
          if (entry.flag && *entry.flag != 't') {
            resultnode->assign(binfromhex(entry.node));
            *resultflag = *entry.flag;
            Py_DECREF(rawobj);
            return;
          } else {
            // Found a directory when expecting a file - give up
            break;
          }
        }

        // If there's more in the query, either recurse or give up
        if (entry.flag && *entry.flag == 't') {
          curnode.assign(binfromhex(entry.node));
          recurse = true;
          break;
        } else {
          // Found a file when we expected a directory
          break;
        }
      }
    }

    Py_DECREF(rawobj);
    if (!recurse) {
      // Failed to find a match
      return;
    }
  }
}

/* Implementation of treemanifest.find()
 * Takes a filename and returns a tuple of the binary hash and flag,
 * or (None, None) if it doesn't exist.
 * */
static PyObject *treemanifest_find(PyObject *o, PyObject *args) {
  treemanifest *self = (treemanifest*)o;
  char *filename;
  Py_ssize_t filenamelen;

  if (!PyArg_ParseTuple(args, "s#", &filename, &filenamelen)) {
    return NULL;
  }

  PyObject *get = PyObject_GetAttrString(self->store, "get");

  std::string resultnode;
  char resultflag;
  _treemanifest_find(std::string(filename, filenamelen), self->node, get, &resultnode, &resultflag);

  Py_DECREF(get);

  if (resultnode.empty()) {
    if (PyErr_Occurred()) {
      return NULL;
    }
    return Py_BuildValue("s#s#", NULL, (Py_ssize_t)0, NULL, (Py_ssize_t)0);
  } else {
    int flaglen = 0;
    if (resultflag != '\n') {
      flaglen = 1;
    }
    return Py_BuildValue("s#s#", resultnode.c_str(), (Py_ssize_t)resultnode.length(), &resultflag, (Py_ssize_t)flaglen);
  }
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
  Py_ssize_t nodelen;

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

/* Pops the data and location entries on the iter stack, for all stack entries
 * that we've already fully processed.
 *
 * Returns false if we've reached the end, or true if there's more work.
 * */
static bool fileiter_popfinished(stackiter *iter) {
  PyObject *rawobj = iter->data.back();
  char *raw;
  Py_ssize_t rawsize;
  PyString_AsStringAndSize(rawobj, &raw, &rawsize);

  char *entrystart = iter->location.back();

  // Pop the stack of trees until we find one we haven't finished iterating
  // over.
  while (entrystart >= raw + rawsize) {
    Py_DECREF(iter->data.back());
    iter->data.pop_back();
    iter->location.pop_back();
    if (iter->data.empty()) {
      // No more directories to pop means we've reached the end of the root
      return false;
    }

    // Pop the top of the path off, to match the newly popped tree stack.
    size_t found = iter->path.rfind('/', iter->path.size() - 2);
    if (found != std::string::npos) {
      iter->path.erase(found + 1);
    } else {
      iter->path.erase(0);
    }

    rawobj = iter->data.back();
    PyString_AsStringAndSize(rawobj, &raw, &rawsize);

    entrystart = iter->location.back();
  }

  return true;
}

/* Returns the next object in the iteration.
 * */
static PyObject *fileiter_iterentriesnext(fileiter *self) {
  stackiter &iter = self->iter;

  // Iterate over the current directory contents
  while (true) {
    // Pop off any directories that we're done processing
    if (!fileiter_popfinished(&iter)) {
      // popfinished returning false means we've finished processing
      return NULL;
    }

    PyObject *rawobj = iter.data.back();
    char *raw;
    Py_ssize_t rawsize;
    PyString_AsStringAndSize(rawobj, &raw, &rawsize);

    // `entrystart` represents the location of the current item in the raw tree data
    // we're iterating over.
    char *entrystart = iter.location.back();
    manifestentry entry = parseentry(entrystart);

    // Move to the next entry for next time
    iter.location[iter.location.size() - 1] = entry.nextentrystart;

    // If a directory, push it and loop again
    if (entry.flag && *entry.flag == 't') {
      iter.path.append(entry.filename, entry.filenamelen);
      iter.path.append(1, '/');

      // Fetch the directory contents
      PyObject *subrawobj = getdata(iter.get, iter.path,
                                    binfromhex(entry.node));
      if (!subrawobj) {
        return NULL;
      }

      // Push the new directory on the stack
      char *subraw;
      Py_ssize_t subrawsize;
      PyString_AsStringAndSize(subrawobj, &subraw, &subrawsize);
      iter.data.push_back(subrawobj);
      iter.location.push_back(subraw);
    } else {
      // If a file, yield it
      int oldpathsize = iter.path.size();
      iter.path.append(entry.filename, entry.filenamelen);
      PyObject* result = PyString_FromStringAndSize(iter.path.c_str(), iter.path.length());
      if (!result) {
        PyErr_NoMemory();
        return NULL;
      }

      iter.path.erase(oldpathsize);
      return result;
    }
  }
}

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
  {"find", treemanifest_find, METH_VARARGS, "returns the node and flag for the given filepath\n"},
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
