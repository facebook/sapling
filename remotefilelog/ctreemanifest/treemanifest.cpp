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

#include "convert.h"

// this is necessary to explicitly call the destructor on clang compilers (see
// https://llvm.org/bugs/show_bug.cgi?id=12350).
using std::string;

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

class Manifest;

/**
 * Class representing the contents of an in memory manifest (versus one that
 * came from the data store). Each in memory instance is the owner of its
 * children in-memory manifests and will delete them during destruction.
 */
class InMemoryManifest {
  private:
    char *_rawstr;
    size_t _rawsize;
    std::vector<InMemoryManifest*> _children;
  public:
    InMemoryManifest() :
      _rawstr(NULL) {
    }

    ~InMemoryManifest() {
      for (size_t i = 0; i < this->_children.size(); i++) {
        InMemoryManifest *child = this->_children[i];
        delete(child);
      }
      delete(this->_rawstr);
    }

    char *getdata() const {
      return this->_rawstr;
    }

    size_t getdatasize() const {
      return this->_rawsize;
    }

    const std::vector<InMemoryManifest*> &children() const {
      return this->_children;
    }
};

/**
 * A key which can be used to look up a manifest, either from the store, or
 * from in memory.
 */
struct manifestkey {
  string *path;
  string *node;
  const InMemoryManifest *memmanifest;

  manifestkey(string *path, string *node,
              const InMemoryManifest *memmanifest) :
    path(path),
    node(node),
    memmanifest(memmanifest) {
  }
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
    char *nextentrystart;
    size_t index;

    // Optional pointer to the in memory child
    const InMemoryManifest *child;
    // TODO: add hint storage here as well

    ManifestEntry() {
      this->filename = NULL;
      filenamelen = 0;
      this->node = NULL;
      this->flag = NULL;
      this->nextentrystart = NULL;
      this->index = -1;
      this->child = NULL;
    }

    /**
     * Given the start of a file/dir entry in a manifest, returns a
     * ManifestEntry structure with the parsed data.
     */
    ManifestEntry(char *entrystart, size_t index, const InMemoryManifest *child = NULL) :
      index(index),
      child(child) {
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

    bool isdirectory() const {
      return this->flag && *this->flag == 't';
    }

    void appendtopath(string &path) {
      path.append(this->filename, this->filenamelen);
      if (this->isdirectory()) {
        path.append(1, '/');
      }
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

    // Optional: If this is set, this manifest refers to an in-memory-only
    // pending manifest (instead of the Python string).
    const InMemoryManifest *_memmanifest;

    char *_firstentry;
    Py_ssize_t _rawsize;
  public:
    Manifest() :
      _memmanifest(NULL),
      _firstentry(NULL),
      _rawsize(0) {
    }

    Manifest(PythonObj &rawobj) :
      _rawobj(rawobj),
      _memmanifest(NULL) {
      PyString_AsStringAndSize(this->_rawobj, &this->_firstentry, &this->_rawsize);
    }

    Manifest(const InMemoryManifest *memmanifest) :
      _memmanifest(memmanifest),
      // TODO: if the memmanifest reallocates its data, this pointer would be
      // out of date. We should either A) allow the Manifest view to always see
      // the latest InMemoryManifest data, or B) detect when the data is no
      // longer up-to-date and block further operations on this view.
      _firstentry(memmanifest->getdata()),
      _rawsize(memmanifest->getdatasize()) {
    }

    bool empty() const {
      return !this->_rawobj && !this->_memmanifest;
    }

    /**
     * Returns the first ManifestEntry in this manifest. The nextentry function
     * can then be used to continue iterating.
     */
    ManifestEntry firstentry() const {
      InMemoryManifest *child = NULL;
      if (this->_memmanifest) {
        const std::vector<InMemoryManifest*> children = this->_memmanifest->children();
        if (children.size() > 0) {
          child = children[0];
        }
      }
      return ManifestEntry(this->_firstentry, 0, child);
    }

    /**
     * Returns the ManifestEntry that follows the provided entry. If we're at
     * the end of the chain, an error is thrown.
     */
    ManifestEntry nextentry(const ManifestEntry &entry) const {
      if (this->islastentry(entry)) {
        throw std::logic_error("called nextentry on the last entry");
      }

      size_t index = entry.index + 1;
      InMemoryManifest *child = NULL;
      if (this->_memmanifest) {
        const std::vector<InMemoryManifest*> children = this->_memmanifest->children();
        if (children.size() > index) {
          child = children[index];
        }
      }

      return ManifestEntry(entry.nextentrystart, index, child);
    }

    /**
     * Returns true if the given ManifestEntry is the last entry in this
     * Manifest.
     */
    bool islastentry(const ManifestEntry &entry) const {
      return entry.nextentrystart >= this->_firstentry + this->_rawsize;
    }
};

/**
 * Class that represents an iterator over the entries of an individual
 * manifest.
 */
class ManifestIterator {
  private:
    Manifest _manifest;
    ManifestEntry _current;
  public:
    ManifestIterator() {
    }

    ManifestIterator(const Manifest &manifest) :
      _manifest(manifest),
      _current(manifest.firstentry()) {
    }

    bool next(ManifestEntry *nextentry) {
      if (this->isfinished()) {
        return false;
      }

      *nextentry = this->_current;

      if (this->_manifest.islastentry(this->_current)) {
        this->_current = ManifestEntry();
      } else {
        this->_current = this->_manifest.nextentry(this->_current);
      }
      return true;
    }

    ManifestEntry currentvalue() const {
      if (this->isfinished()) {
        throw std::logic_error("iterator has no current value");
      }

      return this->_current;
    }

    bool isfinished() const {
      return this->_manifest.empty() || this->_current.filename == NULL;
    }
};

/**
 * Class used to obtain Manifests, given a path and node.
 */
class ManifestFetcher {
  private:
    PythonObj _get;
  public:
    ManifestFetcher(PythonObj &store) :
      _get(store.getattr("get")) {
    }

  /**
   * Fetches the Manifest from the store for the provided manifest key.
   * Returns the manifest if found, or throws an exception if not found.
   */
  Manifest get(const manifestkey &key) const {
    if (key.memmanifest) {
      return Manifest(key.memmanifest);
    }

    PythonObj arglist = Py_BuildValue("s#s#", key.path->c_str(), (Py_ssize_t)key.path->size(),
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
    return Manifest(resultobj);
  }
};

/**
 * A single instance of a treemanifest.
 */
struct treemanifest {
  PyObject_HEAD;

  // A reference to the store that is used to fetch new content
  PythonObj store;

  // The 20-byte root node of this manifest
  string node;

  // Optional in memory root of the tree
  InMemoryManifest *root;
};

/**
 * Represents a single stack frame in an iteration of the contents of the tree.
 */
struct stackframe {
  Manifest manifest;
  ManifestIterator iterator;

  stackframe(const Manifest &manifest) :
    manifest(manifest),
    iterator(manifest) {
  }
};

/**
 * A helper struct representing the state of an iterator recursing over a tree.
 */
struct stackiter {
  // FIXME: This should be a reference to the C++ tree object, not the python
  // tree object.
  const treemanifest *treemf;
  ManifestFetcher fetcher;      // Instance to fetch tree content
  std::vector<stackframe> frames;
  string path;             // The fullpath for the top entry in the stack.

  stackiter(const treemanifest *treemf, ManifestFetcher fetcher) :
    treemf(treemf),
    fetcher(fetcher) {

  }

  stackiter(const stackiter &old) :
    treemf(old.treemf),
    fetcher(old.fetcher),
    frames(old.frames),
    path(old.path) {
      Py_INCREF(this->treemf);
  }

  stackiter& operator=(const stackiter &other) {
    Py_DECREF(this->treemf);
    this->treemf = other.treemf;
    Py_INCREF(this->treemf);

    this->fetcher = other.fetcher;
    this->frames = other.frames;
    this->path = other.path;

    return *this;
  }
};

/**
 * The python iteration object for iterating over a tree.  This is separate from
 * the stackiter above because it lets us just call the constructor on
 * stackiter, which will automatically populate all the members of stackiter.
 */
struct fileiter {
  PyObject_HEAD;
  stackiter iter;

  // A reference to the tree is kept, so it is not freed while we're iterating
  // over it.
  const treemanifest *treemf;

  fileiter(const treemanifest *treemanifest, ManifestFetcher fetcher) :
    iter(treemanifest, fetcher) {
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

// ==== treemanifest functions ====

/**
 * Implementation of treemanifest.__iter__
 * Returns a PyObject iterator instance.
 */
static PyObject *treemanifest_getkeysiter(treemanifest *self) {
  fileiter *i = PyObject_New(fileiter, &fileiterType);
  if (i) {
    try {
      i->treemf = self;
      Py_INCREF(i->treemf);

      ManifestFetcher fetcher(self->store);
      // The provided fileiter struct hasn't initialized our stackiter member, so
      // we do it manually.
      new (&i->iter) stackiter(self, fetcher);

      // Grab the root node's data and prep the iterator
      string rootpath;
      Manifest root = fetcher.get(manifestkey(&rootpath, &self->node, self->root));
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
    string path;
    size_t position;
  public:
    PathIterator(string path) {
      this->path = path;
      this->position = 0;
    }

    bool next(char const ** word, size_t *wordlen) {
      if (this->isfinished()) {
        return false;
      }

      *word = this->path.c_str() + this->position;
      size_t slashoffset = this->path.find('/', this->position);
      if (slashoffset == string::npos) {
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
static PythonObj treemanifest_diffentry(const string *anode, const char *aflag,
                                        const string *bnode, const char *bflag) {
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

/**
 * Simple class for representing a single diff between two files in the
 * manifest.
 */
class DiffEntry {
  private:
    const string *selfnode;
    const string *othernode;
    const char *selfflag;
    const char *otherflag;
  public:
    DiffEntry(const string *selfnode, const char *selfflag,
              const string *othernode, const char *otherflag) {
      this->selfnode = selfnode;
      this->othernode = othernode;
      this->selfflag = selfflag;
      this->otherflag = otherflag;
    }

    void addtodiff(const PythonObj &diff, const string &path) {
      PythonObj entry = treemanifest_diffentry(this->selfnode, this->selfflag,
                                               this->othernode, this->otherflag);
      PythonObj pathObj = PyString_FromStringAndSize(path.c_str(), path.length());

      PyDict_SetItem(diff, pathObj, entry);
    }
};

/**
 * Helper function that performs the actual recursion on the tree entries.
 */
static void treemanifest_diffrecurse(manifestkey *selfkey, manifestkey *otherkey,
                                     const PythonObj &diff, const ManifestFetcher &fetcher) {
  Manifest selfmf;
  Manifest othermf;
  ManifestIterator selfiter;
  ManifestIterator otheriter;

  string *path = NULL;
  if (selfkey) {
    selfmf = fetcher.get(*selfkey);
    selfiter = ManifestIterator(selfmf);
    path = selfkey->path;
  }

  if (otherkey) {
    othermf = fetcher.get(*otherkey);
    otheriter = ManifestIterator(othermf);
    path = otherkey->path;
  }

  // Iterate through both directory contents
  while (!selfiter.isfinished() || !otheriter.isfinished()) {
    int cmp = 0;

    ManifestEntry selfentry;
    string selfbinnode;
    if (!selfiter.isfinished()) {
      cmp--;
      selfentry = selfiter.currentvalue();
      selfbinnode = binfromhex(selfentry.node);
    }

    ManifestEntry otherentry;
    string otherbinnode;
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
        manifestkey selfkey(path, &selfbinnode, selfentry.child);
        treemanifest_diffrecurse(&selfkey, NULL, diff, fetcher);
      } else {
        DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
        entry.addtodiff(diff, *path);
      }
      selfiter.next(&selfentry);
    } else if (cmp > 0) {
      // otherentry should be processed first and only exists in other
      otherentry.appendtopath(*path);
      if (otherentry.isdirectory()) {
        manifestkey otherkey(path, &otherbinnode, otherentry.child);
        treemanifest_diffrecurse(NULL, &otherkey, diff, fetcher);
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
          manifestkey selfkey(path, &selfbinnode, selfentry.child);
          manifestkey otherkey(path, &otherbinnode, otherentry.child);
          treemanifest_diffrecurse(&selfkey, &otherkey, diff, fetcher);
        }
        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      } else if (selfentry.isdirectory() && !otherentry.isdirectory()) {
        // self is directory, other is not - process other then self
        otherentry.appendtopath(*path);
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry.flag);
        entry.addtodiff(diff, *path);

        path->append(1, '/');
        manifestkey selfkey(path, &selfbinnode, selfentry.child);
        treemanifest_diffrecurse(&selfkey, NULL, diff, fetcher);

        selfiter.next(&selfentry);
        otheriter.next(&otherentry);
      } else if (!selfentry.isdirectory() && otherentry.isdirectory()) {
        // self is not directory, other is - process self then other
        selfentry.appendtopath(*path);
        DiffEntry entry(&selfbinnode, selfentry.flag, NULL, NULL);
        entry.addtodiff(diff, *path);

        path->append(1, '/');
        manifestkey otherkey(path, &otherbinnode, otherentry.child);
        treemanifest_diffrecurse(NULL, &otherkey, diff, fetcher);

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
  treemanifest *self = (treemanifest*)o;
  PyObject *otherObj;

  if (!PyArg_ParseTuple(args, "O", &otherObj)) {
    return NULL;
  }

  treemanifest *other = (treemanifest*)otherObj;

  PythonObj results = PyDict_New();

  ManifestFetcher fetcher(self->store);

  string path;
  try {
    path.reserve(1024);
    manifestkey selfkey(&path, &self->node, self->root);
    manifestkey otherkey(&path, &other->node, other->root);
    treemanifest_diffrecurse(&selfkey, &otherkey, results, fetcher);
  } catch (const pyexception &ex) {
    // Python has already set the error message
    return NULL;
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return NULL;
  }

  return results.returnval();
}

static void _treemanifest_find(const string &filename, const manifestkey &root,
    const ManifestFetcher &fetcher, string *resultnode, char *resultflag) {
  // Pre-allocate our curkey so we can reuse it for each iteration
  string curname(*root.path);
  curname.reserve(1024);
  string curnode(*root.node);
  manifestkey curkey(&curname, &curnode, root.memmanifest);

  // Loop over the parts of the query filename
  PathIterator pathiter(filename);
  const char *word;
  size_t wordlen;
  while (pathiter.next(&word, &wordlen)) {
    // Obtain the raw data for this directory
    Manifest manifest = fetcher.get(curkey);

    ManifestIterator mfiterator(manifest);
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
          curkey.memmanifest = entry.child;
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
  treemanifest *self = (treemanifest*)o;
  char *filename;
  Py_ssize_t filenamelen;

  if (!PyArg_ParseTuple(args, "s#", &filename, &filenamelen)) {
    return NULL;
  }

  ManifestFetcher fetcher(self->store);

  string resultnode;
  char resultflag;
  try {
    string rootpath;
    manifestkey rootkey(&rootpath, &self->node, self->root);
    _treemanifest_find(string(filename, filenamelen), rootkey, fetcher,
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
static void treemanifest_dealloc(treemanifest *self){
  self->node.~string();
  self->store.~PythonObj();
  PyObject_Del(self);
}

/*
 * Initializes the contents of a treemanifest
 */
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
    new (&self->node) string(node, nodelen);
  } catch (const std::exception &ex) {
    PyErr_SetString(PyExc_RuntimeError, ex.what());
    return -1;
  }

  return 0;
}

// ==== fileiter functions ====

/**
 * Destructor for the file iterator. Cleans up all the member data of the
 * iterator.
 */
static void fileiter_dealloc(fileiter *self) {
  self->iter.~stackiter();
  Py_XDECREF(self->treemf);
  PyObject_Del(self);
}

/**
 * Pops the data and location entries on the iter stack, for all stack entries
 * that we've already fully processed.
 *
 * Returns false if we've reached the end, or true if there's more work.
 */
static bool fileiter_popfinished(stackiter *iter) {
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
    if (found != string::npos) {
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

      stackframe &frame = iter.frames.back();
      ManifestIterator &iterator = frame.iterator;

      ManifestEntry entry;
      iterator.next(&entry);

      // If a directory, push it and loop again
      if (entry.isdirectory()) {
        iter.path.append(entry.filename, entry.filenamelen);
        iter.path.append(1, '/');

        string binnode = binfromhex(entry.node);
        manifestkey key(&iter.path, &binnode, entry.child);
        Manifest submanifest = iter.fetcher.get(key);
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
