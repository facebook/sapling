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
#include <stdexcept>
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

/* C++ exception that represents an issue at the python C api level.
 * When this is thrown, it's assumed that the python error message has been set
 * and that the catcher of the exception should just return an error code value
 * to the python API.
 * */
class pyexception : public std::exception {
  public:
    pyexception() {
    }
};

/* Wrapper class for PyObject pointers.
 * It is responsible for managing the Py_INCREF and Py_DECREF calls.
 **/
class PythonObj {
  private:
    PyObject *obj;
  public:
    PythonObj(PyObject *obj) {
      if (!obj) {
        if (!PyErr_Occurred()) {
          PyErr_SetString(PyExc_RuntimeError,
              "attempted to construct null PythonObj");
        }
        throw pyexception();
      }
      this->obj = obj;
    }

    PythonObj(const PythonObj& other) {
      this->obj = other.obj;
      Py_INCREF(this->obj);
    }

    ~PythonObj() {
      Py_DECREF(this->obj);
      this->obj = NULL;
    }

    PythonObj& operator=(const PythonObj &other) {
      Py_DECREF(this->obj);
      this->obj = other.obj;
      Py_INCREF(this->obj);
      return *this;
    }

    operator PyObject* () const {
      return this->obj;
    }

    /* Function used to obtain a return value that will persist beyond the life
     * of the PythonObj. This is useful for returning objects to Python C apis
     * and letting them manage the remaining lifetime of the object.
     **/
    PyObject *returnval() {
      Py_INCREF(this->obj);
      return this->obj;
    }

    /* Get's the attribute from the python object.
     **/
    PythonObj getattr(const char *name) {
      return PyObject_GetAttrString(this->obj, name);
    }
};

/*
 * A single instance of a treemanifest.
 * */
typedef struct {
  PyObject_HEAD;

  // A reference to the store that is used to fetch new content
  PythonObj store;

  // The 20-byte root node of this manifest
  std::string node;
} treemanifest;

class ManifestEntry {
  public:
    char *filename;
    size_t filenamelen;
    char *node;
    char *flag;
    char *nextentrystart;

    ManifestEntry() {
      this->filename = NULL;
      filenamelen = 0;
      this->node = NULL;
      this->flag = NULL;
      this->nextentrystart = NULL;
    }

    /* Given the start of a file/dir entry in a manifest, returns a
     * ManifestEntry structure with the parsed data.
     * */
    ManifestEntry(char *entrystart) {
      // Each entry is of the format:
      //
      //   <filename>\0<40-byte hash><optional 1 byte flag>\n
      //
      // Where flags can be 't' to represent a sub directory
      this->filename = entrystart;
      char *nulldelimiter = strchr(entrystart, '\0');
      this->filenamelen = nulldelimiter - entrystart;

      this->node = nulldelimiter + 1;

      this->flag = nulldelimiter + 41;
      if (*this->flag != '\n') {
        this->nextentrystart = this->flag + 2;
      } else {
        // No flag
        this->nextentrystart = this->flag + 1;
        this->flag = NULL;
      }
    }

    bool isdirectory() {
      return this->flag && *this->flag == 't';
    }

    void appendtopath(std::string &path) {
      path.append(this->filename, this->filenamelen);
      if (this->isdirectory()) {
        path.append(1, '/');
      }
    }
};

/*
 * A helper struct representing the state of an iterator recursing over a tree.
 * */
struct stackiter {
  PythonObj get;                // Function to fetch tree content
  std::vector<PythonObj> data;  // Tree content for previous entries in the stack
  std::vector<char*> location;    // The current iteration position for each stack entry
  std::string path;             // The fullpath for the top entry in the stack.

  stackiter(PythonObj get) : get(get) {
  }
};

/*
 * The python iteration object for iterating over a tree.  This is separate from
 * the stackiter above because it lets us just call the constructor on
 * stackiter, which will automatically populate all the members of stackiter.
 * */
struct fileiter {
  PyObject_HEAD;
  stackiter iter;

  fileiter(PythonObj get) : iter(get) {
  }
};

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
static PythonObj getdata(const PythonObj &get, const std::string &dir, const std::string &node) {
  PythonObj arglist = Py_BuildValue("s#s#", dir.c_str(), (Py_ssize_t)dir.size(),
                                            node.c_str(), (Py_ssize_t)node.size());

  PyObject *result = PyEval_CallObject(get, arglist);

  if (!result) {
    if (PyErr_Occurred()) {
      throw pyexception();
    }

    PyErr_Format(PyExc_RuntimeError, "unable to find tree '%s:...'", dir.c_str());
    throw pyexception();
  }

  return PythonObj(result);
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

    bool next(ManifestEntry *entry) {
      if (this->isfinished()) {
        return false;
      }

      *entry = ManifestEntry(this->entrystart);
      this->entrystart = entry->nextentrystart;
      return true;
    }

    ManifestEntry currentvalue() const {
      if (this->isfinished()) {
        throw std::logic_error("iterator has no current value");
      }
      return ManifestEntry(this->entrystart);
    }

    bool isfinished() const {
      return this->raw == NULL || (this->entrystart - this->raw >= this->length);
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
      // Keep a copy of the store's get function for accessing contents
      PythonObj get = self->store.getattr("get");
      // The provided fileiter struct hasn't initialized our stackiter member, so
      // we do it manually.
      new (&i->iter) stackiter(get);

      // Grab the root node's data and prep the iterator
      PythonObj rawobj = getdata(i->iter.get, "", self->node);

      char *raw;
      Py_ssize_t rawsize;
      PyString_AsStringAndSize(rawobj, &raw, &rawsize);

      i->iter.data.push_back(rawobj);
      i->iter.location.push_back(raw);
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

/* Constructs a result python tuple of the given diff data.
 * */
static PythonObj treemanifest_diffentry(const std::string *anode, const char *aflag,
                                        const std::string *bnode, const char *bflag) {
  int aflaglen = 1;
  if (aflag == NULL) {
    aflaglen = 0;
  }
  int bflaglen = 1;
  if (bflag == NULL) {
    bflaglen = 0;
  }
  const char *astr = anode != NULL ? anode->c_str() : NULL;
  Py_ssize_t alen = anode != NULL ? anode->length() : 0;
  const char *bstr = bnode != NULL ? bnode->c_str() : NULL;
  Py_ssize_t blen = bnode != NULL ? bnode->length() : 0;
  PythonObj result = Py_BuildValue("((s#s#)(s#s#))", astr, alen, aflag, Py_ssize_t(aflag ? 1 : 0),
                                                     bstr, blen, bflag, Py_ssize_t(bflag ? 1 : 0));
  return result;
}

/* Simple class for representing a single diff between two files in the
 * manifest.
 * */
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

/* Helper function that performs the actual recursion on the tree entries.
 * */
static void treemanifest_diffrecurse(std::string &path, const std::string *node1, const std::string *node2,
                                     const PythonObj &diff, const PythonObj &get) {
  char *selfraw = NULL;
  Py_ssize_t selfrawsize = 0;

  char *otherraw = NULL;
  Py_ssize_t otherrawsize = 0;

  try {
    if (node1 != NULL) {
      PythonObj selfrawobj = getdata(get, path, *node1);
      PyString_AsStringAndSize(selfrawobj, &selfraw, &selfrawsize);
    }

    if (node2 != NULL) {
      PythonObj otherrawobj = getdata(get, path, *node2);
      PyString_AsStringAndSize(otherrawobj, &otherraw, &otherrawsize);
    }

    // It's ok if these receive a NULL pointer. They treat it as an empty
    // manifest.
    ManifestIterator selfiter(selfraw, selfrawsize);
    ManifestIterator otheriter(otherraw, otherrawsize);

    // Iterate through both directory contents
    while (!selfiter.isfinished() || !otheriter.isfinished()) {
      int cmp = 0;

      ManifestEntry selfentry;
      std::string selfbinnode;
      if (!selfiter.isfinished()) {
        cmp--;
        selfentry = selfiter.currentvalue();
        selfbinnode = binfromhex(selfentry.node);
      }

      ManifestEntry otherentry;
      std::string otherbinnode;
      if (!otheriter.isfinished()) {
        cmp++;
        otherentry = otheriter.currentvalue();
        otherbinnode = binfromhex(otherentry.node);
      }

      // If both sides are present, cmp == 0, so do a filename comparison
      if (cmp == 0) {
        cmp = strcmp(selfentry.filename, otherentry.filename);
      }

      int originalpathsize = path.size();
      if (cmp < 0) {
        // selfentry should be processed first and only exists in self
        selfentry.appendtopath(path);
        if (selfentry.isdirectory()) {
          treemanifest_diffrecurse(path, &selfbinnode, NULL, diff, get);
        } else {
          DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
          entry.addtodiff(diff, path);
        }
        selfiter.next(&selfentry);
      } else if (cmp > 0) {
        // otherentry should be processed first and only exists in other
        otherentry.appendtopath(path);
        if (otherentry.isdirectory()) {
          treemanifest_diffrecurse(path, NULL, &otherbinnode, diff, get);
        } else {
          DiffEntry entry(NULL, NULL, &otherbinnode, otherentry.flag);
          entry.addtodiff(diff, path);
        }
        otheriter.next(&otherentry);
      } else {
        // Filenames match - now compare directory vs file
        if (selfentry.isdirectory() && otherentry.isdirectory()) {
          // Both are directories - recurse
          selfentry.appendtopath(path);

          if (selfbinnode != otherbinnode) {
            treemanifest_diffrecurse(path, &selfbinnode, &otherbinnode, diff, get);
          }
          selfiter.next(&selfentry);
          otheriter.next(&otherentry);
        } else if (selfentry.isdirectory() && !otherentry.isdirectory()) {
          // self is directory, other is not - process other then self
          otherentry.appendtopath(path);
          DiffEntry entry(NULL, NULL, &otherbinnode, otherentry.flag);
          entry.addtodiff(diff, path);

          path.append(1, '/');
          treemanifest_diffrecurse(path, &selfbinnode, NULL, diff, get);

          selfiter.next(&selfentry);
          otheriter.next(&otherentry);
        } else if (!selfentry.isdirectory() && otherentry.isdirectory()) {
          // self is not directory, other is - process self then other
          selfentry.appendtopath(path);
          DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
          entry.addtodiff(diff, path);

          path.append(1, '/');
          treemanifest_diffrecurse(path, NULL, &otherbinnode, diff, get);

          selfiter.next(&selfentry);
          otheriter.next(&otherentry);
        } else {
          // both are files
          bool flagsdiffer = (
            (selfentry.flag && otherentry.flag && *selfentry.flag != *otherentry.flag) ||
            ((bool)selfentry.flag != (bool)selfentry.flag)
          );

          if (selfbinnode != otherbinnode || flagsdiffer) {
            selfentry.appendtopath(path);
            DiffEntry entry(&selfbinnode, selfentry.flag, &otherbinnode, otherentry.flag);
            entry.addtodiff(diff, path);
          }

          selfiter.next(&selfentry);
          otheriter.next(&otherentry);
        }
      }
      path.erase(originalpathsize);
    }
  } catch (const std::exception &ex){
    throw;
  }
}

static PyObject *treemanifest_diff(PyObject *o, PyObject *args) {
  treemanifest *self = (treemanifest*)o;
  PyObject *otherObj;

  if (!PyArg_ParseTuple(args, "O", &otherObj)) {
    return NULL;
  }

  treemanifest *other = (treemanifest*)otherObj;

  PythonObj results = PyDict_New();

  PythonObj get = self->store.getattr("get");

  std::string path;
  try {
    path.reserve(1024);
    treemanifest_diffrecurse(path, &self->node, &other->node, results, get);
  } catch (const pyexception &ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return results.returnval();
}

static void _treemanifest_find(const std::string &filename, const std::string &node,
        const PythonObj &get, std::string *resultnode, char *resultflag) {
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
    PythonObj rawobj = getdata(get, curpath, curnode);

    char* raw;
    Py_ssize_t rawsize;
    PyString_AsStringAndSize(rawobj, &raw, &rawsize);

    ManifestIterator mfiterator(raw, rawsize);
    ManifestEntry entry;
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
          if (!entry.isdirectory()) {
            resultnode->assign(binfromhex(entry.node));
            *resultflag = *entry.flag;
            return;
          } else {
            // Found a directory when expecting a file - give up
            break;
          }
        }

        // If there's more in the query, either recurse or give up
        if (entry.isdirectory()) {
          curnode.assign(binfromhex(entry.node));
          recurse = true;
          break;
        } else {
          // Found a file when we expected a directory
          break;
        }
      }
    }

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

  PythonObj get = self->store.getattr("get");

  std::string resultnode;
  char resultflag;
  try {
    _treemanifest_find(std::string(filename, filenamelen), self->node, get, &resultnode, &resultflag);
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
  self->store.~PythonObj();
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

  Py_INCREF(store);
  new (&self->store) PythonObj(store);

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
  self->iter.~stackiter();
  PyObject_Del(self);
}

/* Pops the data and location entries on the iter stack, for all stack entries
 * that we've already fully processed.
 *
 * Returns false if we've reached the end, or true if there's more work.
 * */
static bool fileiter_popfinished(stackiter *iter) {
  PythonObj rawobj = iter->data.back();
  char *raw;
  Py_ssize_t rawsize;
  PyString_AsStringAndSize(rawobj, &raw, &rawsize);

  char *entrystart = iter->location.back();

  // Pop the stack of trees until we find one we haven't finished iterating
  // over.
  while (entrystart >= raw + rawsize) {
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

  try {
    // Iterate over the current directory contents
    while (true) {
      // Pop off any directories that we're done processing
      if (!fileiter_popfinished(&iter)) {
        // No more directories means we've reached the end of the root
        return NULL;
      }

      PythonObj rawobj = iter.data.back();
      char *raw;
      Py_ssize_t rawsize;
      PyString_AsStringAndSize(rawobj, &raw, &rawsize);

      // `entrystart` represents the location of the current item in the raw tree data
      // we're iterating over.
      char *entrystart = iter.location.back();
      ManifestEntry entry(entrystart);

      // Move to the next entry for next time
      iter.location[iter.location.size() - 1] = entry.nextentrystart;

      // If a directory, push it and loop again
      if (entry.isdirectory()) {
        iter.path.append(entry.filename, entry.filenamelen);
        iter.path.append(1, '/');

        // Fetch the directory contents
        PythonObj subrawobj = getdata(iter.get, iter.path,
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
  } catch (const pyexception &ex) {
    return NULL;
  }
}

// ====  treemanifest ctype declaration ====

static PyMethodDef treemanifest_methods[] = {
  {"diff", treemanifest_diff, METH_VARARGS, "performs a diff of the given two manifests\n"},
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
