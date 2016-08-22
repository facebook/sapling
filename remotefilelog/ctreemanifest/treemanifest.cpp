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
#include <list>
#include <stdexcept>
#include <string>
#include <vector>

#include "convert.h"

/**
 * C++ exception that represents an issue at the python C api level.
 * When this is thrown, it's assumed that the python error message has been set
 * and that the catcher of the exception should just return an error code value
 * to the python API.
 */
class pyexception : public std::exception {
  public:
    pyexception() {
    }
};

/**
 * Wrapper class for PyObject pointers.
 * It is responsible for managing the Py_INCREF and Py_DECREF calls.
 */
class PythonObj {
  private:
    PyObject *obj;
  public:
    PythonObj() :
      obj(NULL) {
    }

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
      Py_XINCREF(this->obj);
    }

    ~PythonObj() {
      Py_XDECREF(this->obj);
    }

    PythonObj& operator=(const PythonObj &other) {
      Py_XDECREF(this->obj);
      this->obj = other.obj;
      Py_XINCREF(this->obj);
      return *this;
    }

    operator PyObject* () const {
      return this->obj;
    }

    /**
     * Function used to obtain a return value that will persist beyond the life
     * of the PythonObj. This is useful for returning objects to Python C apis
     * and letting them manage the remaining lifetime of the object.
     */
    PyObject *returnval() {
      Py_XINCREF(this->obj);
      return this->obj;
    }

    /**
     * Invokes getattr to retrieve the attribute from the python object.
     */
    PythonObj getattr(const char *name) {
      return PyObject_GetAttrString(this->obj, name);
    }
};

/**
 * A key which can be used to look up a manifest.
 */
struct manifestkey {
  std::string *path;
  std::string *node;

  manifestkey(std::string *path, std::string *node) :
      path(path),
      node(node) {
  }
};

class Manifest;
/**
 * Class used to obtain Manifests, given a path and node.
 */
class ManifestFetcher {
private:
  PythonObj _get;
public:
  ManifestFetcher(PythonObj &store);

  /**
   * Fetches the Manifest from the store for the provided manifest key.
   * Returns the manifest if found, or throws an exception if not found.
   */
  Manifest *get(const manifestkey &key) const;
};


/**
 * Class representing a single entry in a given manifest.
 * This class owns none of the memory it points at. It's just a view into a
 * portion of memory someone else owns.
 */
class ManifestEntry {
  public:
    char *filename;
    size_t filenamelen;
    char *node;
    char *flag;
    Manifest *resolved;

    // TODO: add hint storage here as well

    ManifestEntry() {
      this->filename = NULL;
      this->filenamelen = 0;
      this->node = NULL;
      this->flag = NULL;
      this->resolved = NULL;
    }

    /**
     * Given the start of a file/dir entry in a manifest, returns a
     * ManifestEntry structure with the parsed data.
     */
    ManifestEntry(char *&entrystart) {
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
        entrystart = this->flag + 2;
      } else {
        // No flag
        entrystart = this->flag + 1;
        this->flag = NULL;
      }
      this->resolved = NULL;
    }

    bool isdirectory() const {
      return this->flag && *this->flag == 't';
    }

    void appendtopath(std::string &path) {
      path.append(this->filename, this->filenamelen);
      if (this->isdirectory()) {
        path.append(1, '/');
      }
    }

    Manifest *get_manifest(ManifestFetcher fetcher, std::string &path) {
      if (this->resolved == NULL) {
        std::string binnode = binfromhex(node);
        manifestkey key(&path, &binnode);
        this->resolved = fetcher.get(key);
      }

      return this->resolved;
    }
};

/**
 * Class that represents an iterator over the entries of an individual
 * manifest.
 */
class ManifestIterator {
private:
  std::list<ManifestEntry>::const_iterator iterator;
  std::list<ManifestEntry>::const_iterator end;
public:
  ManifestIterator() {
  }

  ManifestIterator(
      std::list<ManifestEntry>::const_iterator iterator,
      std::list<ManifestEntry>::const_iterator end) :
      iterator(iterator), end(end) {
  }

  bool next(ManifestEntry *entry) {
    if (this->isfinished()) {
      return false;
    }

    *entry = *this->iterator;
    this->iterator++;

    return true;
  }

  ManifestEntry currentvalue() const {
    if (this->isfinished()) {
      throw std::logic_error("iterator has no current value");
    }

    return *iterator;
  }

  bool isfinished() const {
    return iterator == end;
  }
};

/**
 * This class represents a view on a particular Manifest instance. It provides
 * access to the list of files/directories at one level of the tree, not the
 * entire tree.
 *
 * Instances of this class do not own the actual storage of manifest data. This
 * class just provides a view onto that existing storage.
 *
 * If the actual manifest data comes from the store, this class refers to it via
 * a PythonObj, and reference counting is used to determine when it's cleaned
 * up.
 *
 * If the actual manifest data comes from an InMemoryManifest, then the life
 * time of that InMemoryManifest is managed elsewhere, and is unaffected by the
 * existence of Manifest objects that view into it.
 */
class Manifest {
  private:
    PythonObj _rawobj;

    std::list<ManifestEntry> entries;
public:
    Manifest() {
    }

    Manifest(PythonObj &rawobj) :
      _rawobj(rawobj) {
      char *parseptr, *endptr;
      Py_ssize_t buf_sz;
      PyString_AsStringAndSize(_rawobj, &parseptr, &buf_sz);
      endptr = parseptr + buf_sz;

      while (parseptr < endptr) {
        ManifestEntry entry = ManifestEntry(parseptr);
        entries.push_back(entry);
      }
    }

    ManifestIterator getIterator() const {
      return ManifestIterator(this->entries.begin(), this->entries.end());
    }
};

////////////////////////////////////////////////////////////////////////////////
// ManifestFetcher implementation
////////////////////////////////////////////////////////////////////////////////

ManifestFetcher::ManifestFetcher(PythonObj &store) :
    _get(store.getattr("get")) {
}

/**
 * Fetches the Manifest from the store for the provided manifest key.
 * Returns the manifest if found, or throws an exception if not found.
 */
Manifest *ManifestFetcher::get(const manifestkey &key) const {
  PythonObj arglist = Py_BuildValue("s#s#",
      key.path->c_str(), (Py_ssize_t)key.path->size(),
      key.node->c_str(), (Py_ssize_t)key.node->size());

  PyObject *result = PyEval_CallObject(this->_get, arglist);

  if (!result) {
    if (PyErr_Occurred()) {
      throw pyexception();
    }

    PyErr_Format(PyExc_RuntimeError, "unable to find tree '%s:...'", key.path->c_str());
    throw pyexception();
  }

  PythonObj resultobj(result);
  return new Manifest(resultobj);
}

/**
 * A single instance of a treemanifest.
 */
struct treemanifest {
  // A reference to the store that is used to fetch new content
  PythonObj store;

  // The 20-byte root node of this manifest
  std::string node;

  treemanifest(PythonObj store, std::string node) :
    store(store),
    node(node) {
  }
};

struct py_treemanifest {
  PyObject_HEAD;

  treemanifest tm;
};

/**
 * Represents a single stack frame in an iteration of the contents of the tree.
 */
struct stackframe {
  const Manifest *manifest;
  ManifestIterator iterator;

  stackframe(const Manifest *manifest) :
    manifest(manifest),
    iterator(manifest->getIterator()) {
  }
};

/**
 * A helper struct representing the state of an iterator recursing over a tree.
 */
struct fileiter {
  ManifestFetcher fetcher;      // Instance to fetch tree content
  std::vector<stackframe> frames;
  std::string path;             // The fullpath for the top entry in the stack.

  fileiter(ManifestFetcher fetcher) :
    fetcher(fetcher) {
  }

  fileiter(const fileiter &old) :
    fetcher(old.fetcher),
    frames(old.frames),
    path(old.path) {
  }

  fileiter& operator=(const fileiter &other) {
    this->fetcher = other.fetcher;
    this->frames = other.frames;
    this->path = other.path;

    return *this;
  }
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
      std::string rootpath;
      Manifest *root = fetcher.get(
          manifestkey(&rootpath, &self->tm.node));

      // TODO: root manifest should be stored in the treemanifest object and
      // used if it's available.
      i->iter.frames.push_back(stackframe(root));

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
    const Manifest *selfmf,
    const Manifest *othermf,
    std::string *path,
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

    int originalpathsize = path->size();
    if (cmp < 0) {
      // selfentry should be processed first and only exists in self
      selfentry.appendtopath(*path);
      if (selfentry.isdirectory()) {
        Manifest *selfchildmanifest = selfentry.get_manifest(fetcher, *path);
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);
      } else {
        DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
        entry.addtodiff(diff, *path);
      }
      selfiter.next(&selfentry);
    } else if (cmp > 0) {
      // otherentry should be processed first and only exists in other
      otherentry.appendtopath(*path);
      if (otherentry.isdirectory()) {
        Manifest *otherchildmanifest = otherentry.get_manifest(fetcher, *path);
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);
      } else {
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry.flag);
        entry.addtodiff(diff, *path);
      }
      otheriter.next(&otherentry);
    } else {
      // Filenames match - now compare directory vs file
      if (selfentry.isdirectory() && otherentry.isdirectory()) {
        // Both are directories - recurse
        selfentry.appendtopath(*path);

        if (selfbinnode != otherbinnode) {
          manifestkey selfkey(path, &selfbinnode);
          manifestkey otherkey(path, &otherbinnode);
          Manifest *selfchildmanifest = fetcher.get(selfkey);
          Manifest *otherchildmanifest = fetcher.get(otherkey);

          treemanifest_diffrecurse(
              selfchildmanifest,
              otherchildmanifest,
              path,
              diff,
              fetcher);
        }
        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      } else if (selfentry.isdirectory() && !otherentry.isdirectory()) {
        // self is directory, other is not - process other then self
        otherentry.appendtopath(*path);
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry.flag);
        entry.addtodiff(diff, *path);

        path->append(1, '/');
        manifestkey selfkey(path, &selfbinnode);
        Manifest *selfchildmanifest = fetcher.get(selfkey);
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);

        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      } else if (!selfentry.isdirectory() && otherentry.isdirectory()) {
        // self is not directory, other is - process self then other
        selfentry.appendtopath(*path);
        DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
        entry.addtodiff(diff, *path);

        path->append(1, '/');
        manifestkey otherkey(path, &otherbinnode);
        Manifest *otherchildmanifest = fetcher.get(otherkey);
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);

        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      } else {
        // both are files
        bool flagsdiffer = (
          (selfentry.flag && otherentry.flag && *selfentry.flag != *otherentry.flag) ||
          ((bool)selfentry.flag != (bool)selfentry.flag)
        );

        if (selfbinnode != otherbinnode || flagsdiffer) {
          selfentry.appendtopath(*path);
          DiffEntry entry(&selfbinnode, selfentry.flag, &otherbinnode, otherentry.flag);
          entry.addtodiff(diff, *path);
        }

        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      }
    }
    path->erase(originalpathsize);
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
    manifestkey selfkey(&path, &self->tm.node);
    manifestkey otherkey(&path, &other->tm.node);
    Manifest *selfmanifest = fetcher.get(selfkey);
    Manifest *othermanifest = fetcher.get(otherkey);
    treemanifest_diffrecurse(
        selfmanifest, othermanifest, &path, results, fetcher);
  } catch (const pyexception &ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return results.returnval();
}

static void _treemanifest_find(const std::string &filename, const manifestkey &root,
    const ManifestFetcher &fetcher, std::string *resultnode, char *resultflag) {
  // Pre-allocate our curkey so we can reuse it for each iteration
  std::string curname(*root.path);
  curname.reserve(1024);
  std::string curnode(*root.node);
  manifestkey curkey(&curname, &curnode);

  // Loop over the parts of the query filename
  PathIterator pathiter(filename);
  const char *word;
  size_t wordlen;
  while (pathiter.next(&word, &wordlen)) {
    // Obtain the raw data for this directory
    Manifest *manifest = fetcher.get(curkey);

    // TODO: need to attach this manifest to the parent Manifest object.

    ManifestIterator mfiterator = manifest->getIterator();
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
            if (entry.flag == NULL) {
              *resultflag = '\0';
            } else {
              *resultflag = *entry.flag;
            }
            return;
          } else {
            // Found a directory when expecting a file - give up
            break;
          }
        }

        // If there's more in the query, either recurse or give up
        size_t nextpathlen = curkey.path->length() + wordlen + 1;
        if (entry.isdirectory() && filename.length() > nextpathlen) {
          // Get the fullpath of the current directory/file we're searching in
          curkey.path->assign(filename, 0, nextpathlen);
          curkey.node->assign(binfromhex(entry.node));
          recurse = true;
          break;
        } else {
          // Found a file when we expected a directory or
          // found a directory when we expected a file.
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
    std::string rootpath;
    manifestkey rootkey(&rootpath, &self->tm.node);
    _treemanifest_find(std::string(filename, filenamelen), rootkey, fetcher,
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

      ManifestEntry entry;
      iterator.next(&entry);

      // If a directory, push it and loop again
      if (entry.isdirectory()) {
        iter.path.append(entry.filename, entry.filenamelen);
        iter.path.append(1, '/');

        Manifest *submanifest = entry.get_manifest(iter.fetcher, iter.path);

        // TODO: memory cleanup here is probably broken.
        iter.frames.push_back(stackframe(submanifest));

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
