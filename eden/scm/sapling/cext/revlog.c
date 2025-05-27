/*
 * Portions Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 parsers.c - efficient content parsing

 Copyright 2008 Olivia Mackall <olivia@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#define PY_SSIZE_T_CLEAN
#include <Python.h> // @manual=fbsource//third-party/python:python
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "eden/scm/sapling/bitmanipulation.h"
#include "eden/scm/sapling/cext/charencode.h"
#include "eden/scm/sapling/cext/util.h"

#ifdef IS_PY3K
/* The mapping of Python types is meant to be temporary to get Python
 * 3 to compile. We should remove this once Python 3 support is fully
 * supported and proper types are used in the extensions themselves. */
#define PyInt_Check PyLong_Check
#define PyInt_FromLong PyLong_FromLong
#define PyInt_FromSsize_t PyLong_FromSsize_t
#define PyInt_AS_LONG PyLong_AS_LONG
#define PyInt_AsLong PyLong_AsLong
#endif

/*
 * A base-16 trie for fast node->rev mapping.
 *
 * Positive value is index of the next node in the trie
 * Negative value is a leaf: -(rev + 1)
 * Zero is empty
 */
typedef struct {
  int children[16];
} nodetree;

/*
 * This class has two behaviors.
 *
 * When used in a list-like way (with integer keys), we decode an
 * entry in a RevlogNG index file on demand. Our last entry is a
 * sentinel, always a nullid.  We have limited support for
 * integer-keyed insert and delete, only at elements right before the
 * sentinel.
 *
 * With string keys, we lazily perform a reverse mapping from node to
 * rev, using a base-16 trie.
 */
typedef struct {
  PyObject_HEAD
      /* Type-specific fields go here. */
      PyObject* data; /* raw bytes of index */
  Py_buffer buf; /* buffer of data */
  PyObject** cache; /* cached tuples */
  const char** offsets; /* populated on demand */
  Py_ssize_t raw_length; /* original number of elements */
  Py_ssize_t length; /* current number of elements */
  PyObject* added; /* populated on demand */
  PyObject* headrevs; /* cache, invalidated on changes */
  nodetree* nt; /* base-16 trie */
  size_t ntlength; /* # nodes in use */
  size_t ntcapacity; /* # nodes allocated */
  int ntdepth; /* maximum depth of tree */
  int ntsplits; /* # splits performed */
  int ntrev; /* last rev scanned */
  int ntlookups; /* # lookups */
  int ntmisses; /* # lookups that miss the cache */
  int inlined;
} indexObject;

static Py_ssize_t index_length(const indexObject* self) {
  if (self->added == NULL)
    return self->length;
  return self->length + PyList_GET_SIZE(self->added);
}

static PyObject* nullentry;
static const char nullid[20];

static Py_ssize_t inline_scan(indexObject* self, const char** offsets);

#ifdef IS_PY3K
#if LONG_MAX == 0x7fffffffL
static char* tuple_format = "Kiiiiiiy#";
#else
static char* tuple_format = "kiiiiiiy#";
#endif
#else
#if LONG_MAX == 0x7fffffffL
static char* tuple_format = "Kiiiiiis#";
#else
static char* tuple_format = "kiiiiiis#";
#endif
#endif

/* A RevlogNG v1 index entry is 64 bytes long. */
static const long v1_hdrsize = 64;

/*
 * Return a pointer to the beginning of a RevlogNG record.
 */
static const char* index_deref(indexObject* self, Py_ssize_t pos) {
  if (self->inlined && pos > 0) {
    if (self->offsets == NULL) {
      self->offsets = PyMem_Malloc(self->raw_length * sizeof(*self->offsets));
      if (self->offsets == NULL)
        return (const char*)PyErr_NoMemory();
      inline_scan(self, self->offsets);
    }
    return self->offsets[pos];
  }

  return (const char*)(self->buf.buf) + pos * v1_hdrsize;
}

static inline int
index_get_parents(indexObject* self, Py_ssize_t rev, int* ps, int maxrev) {
  if (rev >= self->length - 1) {
    PyObject* tuple = PyList_GET_ITEM(self->added, rev - self->length + 1);
    ps[0] = (int)PyInt_AS_LONG(PyTuple_GET_ITEM(tuple, 5));
    ps[1] = (int)PyInt_AS_LONG(PyTuple_GET_ITEM(tuple, 6));
  } else {
    const char* data = index_deref(self, rev);
    ps[0] = getbe32(data + 24);
    ps[1] = getbe32(data + 28);
  }
  /* If index file is corrupted, ps[] may point to invalid revisions. So
   * there is a risk of buffer overflow to trust them unconditionally. */
  if (ps[0] > maxrev || ps[1] > maxrev) {
    PyErr_SetString(PyExc_ValueError, "parent out of range");
    return -1;
  }
  return 0;
}

/*
 * RevlogNG format (all in big endian, data may be inlined):
 *    6 bytes: offset
 *    2 bytes: flags
 *    4 bytes: compressed length
 *    4 bytes: uncompressed length
 *    4 bytes: base revision
 *    4 bytes: link revision
 *    4 bytes: parent 1 revision
 *    4 bytes: parent 2 revision
 *   32 bytes: nodeid (only 20 bytes used)
 */
static PyObject* index_get(indexObject* self, Py_ssize_t pos) {
  uint64_t offset_flags;
  int comp_len, uncomp_len, base_rev, link_rev, parent_1, parent_2;
  const char* c_node_id;
  const char* data;
  Py_ssize_t length = index_length(self);
  PyObject* entry;

  if (pos < 0)
    pos += length;

  if (pos < 0 || pos >= length) {
    PyErr_SetString(PyExc_IndexError, "revlog index out of range");
    return NULL;
  }

  if (pos == length - 1) {
    Py_INCREF(nullentry);
    return nullentry;
  }

  if (pos >= self->length - 1) {
    PyObject* obj;
    obj = PyList_GET_ITEM(self->added, pos - self->length + 1);
    Py_INCREF(obj);
    return obj;
  }

  if (self->cache) {
    if (self->cache[pos]) {
      Py_INCREF(self->cache[pos]);
      return self->cache[pos];
    }
  } else {
    self->cache = calloc(self->raw_length, sizeof(PyObject*));
    if (self->cache == NULL)
      return PyErr_NoMemory();
  }

  data = index_deref(self, pos);
  if (data == NULL)
    return NULL;

  offset_flags = getbe32(data + 4);
  if (pos == 0) /* mask out version number for the first entry */
    offset_flags &= 0xFFFF;
  else {
    uint32_t offset_high = getbe32(data);
    offset_flags |= ((uint64_t)offset_high) << 32;
  }

  comp_len = getbe32(data + 8);
  uncomp_len = getbe32(data + 12);
  base_rev = getbe32(data + 16);
  link_rev = getbe32(data + 20);
  parent_1 = getbe32(data + 24);
  parent_2 = getbe32(data + 28);
  c_node_id = data + 32;

  entry = Py_BuildValue(
      tuple_format,
      offset_flags,
      comp_len,
      uncomp_len,
      base_rev,
      link_rev,
      parent_1,
      parent_2,
      c_node_id,
      (Py_ssize_t)20);

  if (entry) {
    PyObject_GC_UnTrack(entry);
    Py_INCREF(entry);
  }

  self->cache[pos] = entry;

  return entry;
}

/*
 * Return the 20-byte SHA of the node corresponding to the given rev.
 */
static const char* index_node(indexObject* self, Py_ssize_t pos) {
  Py_ssize_t length = index_length(self);
  const char* data;

  if (pos == length - 1 || pos == INT_MAX)
    return nullid;

  if (pos >= length)
    return NULL;

  if (pos >= self->length - 1) {
    PyObject *tuple, *str;
    tuple = PyList_GET_ITEM(self->added, pos - self->length + 1);
    str = PyTuple_GetItem(tuple, 7);
    return str ? PyBytes_AS_STRING(str) : NULL;
  }

  data = index_deref(self, pos);
  return data ? data + 32 : NULL;
}

static int nt_insert(indexObject* self, const char* node, int rev);

static int node_check(PyObject* obj, char** node, Py_ssize_t* nodelen) {
  if (PyBytes_AsStringAndSize(obj, node, nodelen) == -1)
    return -1;
  if (*nodelen == 20)
    return 0;
  PyErr_SetString(PyExc_ValueError, "20-byte hash required");
  return -1;
}

static PyObject* index_insert(indexObject* self, PyObject* args) {
  PyObject* obj;
  char* node;
  int index;
  Py_ssize_t len, nodelen;

  if (!PyArg_ParseTuple(args, "iO", &index, &obj))
    return NULL;

  if (!PyTuple_Check(obj) || PyTuple_GET_SIZE(obj) != 8) {
    PyErr_SetString(PyExc_TypeError, "8-tuple required");
    return NULL;
  }

  if (node_check(PyTuple_GET_ITEM(obj, 7), &node, &nodelen) == -1)
    return NULL;

  len = index_length(self);

  if (index < 0)
    index += len;

  if (index != len - 1) {
    PyErr_SetString(PyExc_IndexError, "insert only supported at index -1");
    return NULL;
  }

  if (self->added == NULL) {
    self->added = PyList_New(0);
    if (self->added == NULL)
      return NULL;
  }

  if (PyList_Append(self->added, obj) == -1)
    return NULL;

  if (self->nt)
    nt_insert(self, node, index);

  Py_CLEAR(self->headrevs);
  Py_RETURN_NONE;
}

static void _index_clearcaches(indexObject* self) {
  if (self->cache) {
    Py_ssize_t i;

    for (i = 0; i < self->raw_length; i++)
      Py_CLEAR(self->cache[i]);
    free(self->cache);
    self->cache = NULL;
  }
  if (self->offsets) {
    PyMem_Free(self->offsets);
    self->offsets = NULL;
  }
  if (self->nt) {
    free(self->nt);
    self->nt = NULL;
  }
  Py_CLEAR(self->headrevs);
}

static PyObject* index_clearcaches(indexObject* self) {
  _index_clearcaches(self);
  self->ntlength = self->ntcapacity = 0;
  self->ntdepth = self->ntsplits = 0;
  self->ntrev = -1;
  self->ntlookups = self->ntmisses = 0;
  Py_RETURN_NONE;
}

static PyObject* index_stats(indexObject* self) {
  PyObject* obj = PyDict_New();
  PyObject* t = NULL;

  if (obj == NULL)
    return NULL;

#define istat(__n, __d)                          \
  do {                                           \
    t = PyInt_FromSsize_t(self->__n);            \
    if (!t)                                      \
      goto bail;                                 \
    if (PyDict_SetItemString(obj, __d, t) == -1) \
      goto bail;                                 \
    Py_DECREF(t);                                \
  } while (0)

  if (self->added) {
    Py_ssize_t len = PyList_GET_SIZE(self->added);
    t = PyInt_FromSsize_t(len);
    if (!t)
      goto bail;
    if (PyDict_SetItemString(obj, "index entries added", t) == -1)
      goto bail;
    Py_DECREF(t);
  }

  if (self->raw_length != self->length - 1)
    istat(raw_length, "revs on disk");
  istat(length, "revs in memory");
  istat(ntcapacity, "node trie capacity");
  istat(ntdepth, "node trie depth");
  istat(ntlength, "node trie count");
  istat(ntlookups, "node trie lookups");
  istat(ntmisses, "node trie misses");
  istat(ntrev, "node trie last rev scanned");
  istat(ntsplits, "node trie splits");

#undef istat

  return obj;

bail:
  Py_XDECREF(obj);
  Py_XDECREF(t);
  return NULL;
}

/*
 * When we cache a list, we want to be sure the caller can't mutate
 * the cached copy.
 */
static PyObject* list_copy(PyObject* list) {
  Py_ssize_t len = PyList_GET_SIZE(list);
  PyObject* newlist = PyList_New(len);
  Py_ssize_t i;

  if (newlist == NULL)
    return NULL;

  for (i = 0; i < len; i++) {
    PyObject* obj = PyList_GET_ITEM(list, i);
    Py_INCREF(obj);
    PyList_SET_ITEM(newlist, i, obj);
  }

  return newlist;
}

static Py_ssize_t add_roots_get_min(
    indexObject* self,
    PyObject* list,
    Py_ssize_t marker,
    char* phases) {
  PyObject* iter = NULL;
  PyObject* iter_item = NULL;
  Py_ssize_t min_idx = index_length(self) + 1;

  long len = index_length(self) - 1;
  long iter_item_long;

  if (PyList_GET_SIZE(list) != 0) {
    iter = PyObject_GetIter(list);
    if (iter == NULL)
      return -2;
    while ((iter_item = PyIter_Next(iter))) {
      iter_item_long = PyInt_AS_LONG(iter_item);
      if (iter_item_long >= len) {
        // Ignore bogus roots
        continue;
      }
      Py_DECREF(iter_item);
      if (iter_item_long < min_idx)
        min_idx = iter_item_long;
      phases[iter_item_long] = marker;
    }
    Py_DECREF(iter);
  }

  return min_idx;
}

static inline void
set_phase_from_parents(char* phases, int parent_1, int parent_2, Py_ssize_t i) {
  if (parent_1 >= 0 && phases[parent_1] > phases[i])
    phases[i] = phases[parent_1];
  if (parent_2 >= 0 && phases[parent_2] > phases[i])
    phases[i] = phases[parent_2];
}

static PyObject* reachableroots2(indexObject* self, PyObject* args) {
  /* Input */
  long minroot;
  PyObject* includepatharg = NULL;
  int includepath = 0;
  /* heads and roots are lists */
  PyObject* heads = NULL;
  PyObject* roots = NULL;
  PyObject* reachable = NULL;

  PyObject* val;
  Py_ssize_t len = index_length(self) - 1;
  long revnum;
  Py_ssize_t k;
  Py_ssize_t i;
  Py_ssize_t l;
  int r;
  int parents[2];

  /* Internal data structure:
   * tovisit: array of length len+1 (all revs + nullrev), filled upto lentovisit
   * revstates: array of length len+1 (all revs + nullrev) */
  int* tovisit = NULL;
  long lentovisit = 0;
  enum { RS_SEEN = 1, RS_ROOT = 2, RS_REACHABLE = 4 };
  char* revstates = NULL;

  /* Get arguments */
  if (!PyArg_ParseTuple(
          args,
          "lO!O!O!",
          &minroot,
          &PyList_Type,
          &heads,
          &PyList_Type,
          &roots,
          &PyBool_Type,
          &includepatharg))
    goto bail;

  if (includepatharg == Py_True)
    includepath = 1;

  /* Initialize return set */
  reachable = PyList_New(0);
  if (reachable == NULL)
    goto bail;

  /* Initialize internal datastructures */
  tovisit = (int*)malloc((len + 1) * sizeof(int));
  if (tovisit == NULL) {
    PyErr_NoMemory();
    goto bail;
  }

  revstates = (char*)calloc(len + 1, 1);
  if (revstates == NULL) {
    PyErr_NoMemory();
    goto bail;
  }

  l = PyList_GET_SIZE(roots);
  for (i = 0; i < l; i++) {
    revnum = PyInt_AsLong(PyList_GET_ITEM(roots, i));
    if (revnum == -1 && PyErr_Occurred())
      goto bail;
    /* If root is out of range, e.g. wdir(), it must be unreachable
     * from heads. So we can just ignore it. */
    if (revnum + 1 < 0 || revnum + 1 >= len + 1)
      continue;
    revstates[revnum + 1] |= RS_ROOT;
  }

  /* Populate tovisit with all the heads */
  l = PyList_GET_SIZE(heads);
  for (i = 0; i < l; i++) {
    revnum = PyInt_AsLong(PyList_GET_ITEM(heads, i));
    if (revnum == -1 && PyErr_Occurred())
      goto bail;
    if (revnum + 1 < 0 || revnum + 1 >= len + 1) {
      PyErr_SetString(PyExc_IndexError, "head out of range");
      goto bail;
    }
    if (!(revstates[revnum + 1] & RS_SEEN)) {
      tovisit[lentovisit++] = (int)revnum;
      revstates[revnum + 1] |= RS_SEEN;
    }
  }

  /* Visit the tovisit list and find the reachable roots */
  k = 0;
  while (k < lentovisit) {
    /* Add the node to reachable if it is a root*/
    revnum = tovisit[k++];
    if (revstates[revnum + 1] & RS_ROOT) {
      revstates[revnum + 1] |= RS_REACHABLE;
      val = PyInt_FromLong(revnum);
      if (val == NULL)
        goto bail;
      r = PyList_Append(reachable, val);
      Py_DECREF(val);
      if (r < 0)
        goto bail;
      if (includepath == 0)
        continue;
    }

    /* Add its parents to the list of nodes to visit */
    if (revnum == -1)
      continue;
    r = index_get_parents(self, revnum, parents, (int)len - 1);
    if (r < 0)
      goto bail;
    for (i = 0; i < 2; i++) {
      if (!(revstates[parents[i] + 1] & RS_SEEN) && parents[i] >= minroot) {
        tovisit[lentovisit++] = parents[i];
        revstates[parents[i] + 1] |= RS_SEEN;
      }
    }
  }

  /* Find all the nodes in between the roots we found and the heads
   * and add them to the reachable set */
  if (includepath == 1) {
    long minidx = minroot;
    if (minidx < 0)
      minidx = 0;
    for (i = minidx; i < len; i++) {
      if (!(revstates[i + 1] & RS_SEEN))
        continue;
      r = index_get_parents(self, i, parents, (int)len - 1);
      /* Corrupted index file, error is set from
       * index_get_parents */
      if (r < 0)
        goto bail;
      if (((revstates[parents[0] + 1] | revstates[parents[1] + 1]) &
           RS_REACHABLE) &&
          !(revstates[i + 1] & RS_REACHABLE)) {
        revstates[i + 1] |= RS_REACHABLE;
        val = PyInt_FromLong(i);
        if (val == NULL)
          goto bail;
        r = PyList_Append(reachable, val);
        Py_DECREF(val);
        if (r < 0)
          goto bail;
      }
    }
  }

  free(revstates);
  free(tovisit);
  return reachable;
bail:
  Py_XDECREF(reachable);
  free(revstates);
  free(tovisit);
  return NULL;
}

static PyObject* compute_phases_map_sets(indexObject* self, PyObject* args) {
  PyObject* roots = Py_None;
  PyObject* ret = NULL;
  PyObject* phasessize = NULL;
  PyObject* phaseroots = NULL;
  PyObject* phaseset = NULL;
  PyObject* phasessetlist = NULL;
  PyObject* rev = NULL;
  Py_ssize_t len = index_length(self) - 1;
  Py_ssize_t numphase = 0;
  Py_ssize_t minrevallphases = 0;
  Py_ssize_t minrevphase = 0;
  Py_ssize_t i = 0;
  char* phases = NULL;
  long phase;

  if (!PyArg_ParseTuple(args, "O", &roots))
    goto done;
  if (roots == NULL || !PyList_Check(roots))
    goto done;

  phases = calloc(len, 1); /* phase per rev: {0: public, 1: draft, 2: secret} */
  if (phases == NULL) {
    PyErr_NoMemory();
    goto done;
  }
  /* Put the phase information of all the roots in phases */
  numphase = PyList_GET_SIZE(roots) + 1;
  minrevallphases = len + 1;
  phasessetlist = PyList_New(numphase);
  if (phasessetlist == NULL)
    goto done;

  PyList_SET_ITEM(phasessetlist, 0, Py_None);
  Py_INCREF(Py_None);

  for (i = 0; i < numphase - 1; i++) {
    phaseroots = PyList_GET_ITEM(roots, i);
    phaseset = PySet_New(NULL);
    if (phaseset == NULL)
      goto release;
    PyList_SET_ITEM(phasessetlist, i + 1, phaseset);
    if (!PyList_Check(phaseroots))
      goto release;
    minrevphase = add_roots_get_min(self, phaseroots, i + 1, phases);
    if (minrevphase == -2) /* Error from add_roots_get_min */
      goto release;
    minrevallphases = MIN(minrevallphases, minrevphase);
  }
  /* Propagate the phase information from the roots to the revs */
  if (minrevallphases != -1) {
    int parents[2];
    for (i = minrevallphases; i < len; i++) {
      if (index_get_parents(self, i, parents, (int)len - 1) < 0)
        goto release;
      set_phase_from_parents(phases, parents[0], parents[1], i);
    }
  }
  /* Transform phase list to a python list */
  phasessize = PyInt_FromLong(len);
  if (phasessize == NULL)
    goto release;
  for (i = 0; i < len; i++) {
    phase = phases[i];
    /* We only store the sets of phase for non public phase, the public phase
     * is computed as a difference */
    if (phase != 0) {
      phaseset = PyList_GET_ITEM(phasessetlist, phase);
      rev = PyInt_FromLong(i);
      if (rev == NULL)
        goto release;
      PySet_Add(phaseset, rev);
      Py_XDECREF(rev);
    }
  }
  ret = PyTuple_Pack(2, phasessize, phasessetlist);

release:
  Py_XDECREF(phasessize);
  Py_XDECREF(phasessetlist);
done:
  free(phases);
  return ret;
}

static PyObject* index_headrevs(indexObject* self, PyObject* args) {
  Py_ssize_t i, j, len;
  char* nothead = NULL;
  PyObject* heads = NULL;

  if (self->headrevs)
    return list_copy(self->headrevs);

  len = index_length(self) - 1;
  heads = PyList_New(0);
  if (heads == NULL)
    goto bail;
  if (len == 0) {
    PyObject* nullid = PyInt_FromLong(-1);
    if (nullid == NULL || PyList_Append(heads, nullid) == -1) {
      Py_XDECREF(nullid);
      goto bail;
    }
    goto done;
  }

  nothead = calloc(len, 1);
  if (nothead == NULL) {
    PyErr_NoMemory();
    goto bail;
  }

  for (i = len - 1; i >= 0; i--) {
    int parents[2];

    if (index_get_parents(self, i, parents, (int)len - 1) < 0)
      goto bail;
    for (j = 0; j < 2; j++) {
      if (parents[j] >= 0)
        nothead[parents[j]] = 1;
    }
  }

  for (i = 0; i < len; i++) {
    PyObject* head;

    if (nothead[i])
      continue;
    head = PyInt_FromSsize_t(i);
    if (head == NULL || PyList_Append(heads, head) == -1) {
      Py_XDECREF(head);
      goto bail;
    }
  }

done:
  self->headrevs = heads;
  free(nothead);
  return list_copy(self->headrevs);
bail:
  Py_XDECREF(heads);
  free(nothead);
  return NULL;
}

/**
 * Obtain the base revision index entry.
 *
 * Callers must ensure that rev >= 0 or illegal memory access may occur.
 */
static inline int index_baserev(indexObject* self, int rev) {
  const char* data;

  if (rev >= self->length - 1) {
    PyObject* tuple = PyList_GET_ITEM(self->added, rev - self->length + 1);
    return (int)PyInt_AS_LONG(PyTuple_GET_ITEM(tuple, 3));
  } else {
    data = index_deref(self, rev);
    if (data == NULL) {
      return -2;
    }

    return getbe32(data + 16);
  }
}

static PyObject* index_deltachain(indexObject* self, PyObject* args) {
  int rev, generaldelta;
  PyObject* stoparg;
  int stoprev, iterrev, baserev = -1;
  int stopped;
  PyObject *chain = NULL, *result = NULL;
  const Py_ssize_t length = index_length(self);

  if (!PyArg_ParseTuple(args, "iOi", &rev, &stoparg, &generaldelta)) {
    return NULL;
  }

  if (PyInt_Check(stoparg)) {
    stoprev = (int)PyInt_AsLong(stoparg);
    if (stoprev == -1 && PyErr_Occurred()) {
      return NULL;
    }
  } else if (stoparg == Py_None) {
    stoprev = -2;
  } else {
    PyErr_SetString(PyExc_ValueError, "stoprev must be integer or None");
    return NULL;
  }

  if (rev < 0 || rev >= length - 1) {
    PyErr_SetString(PyExc_ValueError, "revlog index out of range");
    return NULL;
  }

  chain = PyList_New(0);
  if (chain == NULL) {
    return NULL;
  }

  baserev = index_baserev(self, rev);

  /* This should never happen. */
  if (baserev <= -2) {
    /* Error should be set by index_deref() */
    assert(PyErr_Occurred());
    goto bail;
  }

  iterrev = rev;

  while (iterrev != baserev && iterrev != stoprev) {
    PyObject* value = PyInt_FromLong(iterrev);
    if (value == NULL) {
      goto bail;
    }
    if (PyList_Append(chain, value)) {
      Py_DECREF(value);
      goto bail;
    }
    Py_DECREF(value);

    if (generaldelta) {
      iterrev = baserev;
    } else {
      iterrev--;
    }

    if (iterrev < 0) {
      break;
    }

    if (iterrev >= length - 1) {
      PyErr_SetString(PyExc_IndexError, "revision outside index");
      return NULL;
    }

    baserev = index_baserev(self, iterrev);

    /* This should never happen. */
    if (baserev <= -2) {
      /* Error should be set by index_deref() */
      assert(PyErr_Occurred());
      goto bail;
    }
  }

  if (iterrev == stoprev) {
    stopped = 1;
  } else {
    PyObject* value = PyInt_FromLong(iterrev);
    if (value == NULL) {
      goto bail;
    }
    if (PyList_Append(chain, value)) {
      Py_DECREF(value);
      goto bail;
    }
    Py_DECREF(value);

    stopped = 0;
  }

  if (PyList_Reverse(chain)) {
    goto bail;
  }

  result = Py_BuildValue("OO", chain, stopped ? Py_True : Py_False);
  Py_DECREF(chain);
  return result;

bail:
  Py_DECREF(chain);
  return NULL;
}

static inline int nt_level(const char* node, Py_ssize_t level) {
  int v = node[level >> 1];
  if (!(level & 1))
    v >>= 4;
  return v & 0xf;
}

/*
 * Return values:
 *
 *   -4: match is ambiguous (multiple candidates)
 *   -2: not found
 * rest: valid rev
 */
static int
nt_find(indexObject* self, const char* node, Py_ssize_t nodelen, int hex) {
  int (*getnybble)(const char*, Py_ssize_t) = hex ? hexdigit : nt_level;
  int level, maxlevel, off;

  if (nodelen == 20 && node[0] == '\0' && memcmp(node, nullid, 20) == 0)
    return -1;

  if (self->nt == NULL)
    return -2;

  if (hex)
    maxlevel = nodelen > 40 ? 40 : (int)nodelen;
  else
    maxlevel = nodelen > 20 ? 40 : ((int)nodelen * 2);

  for (level = off = 0; level < maxlevel; level++) {
    int k = getnybble(node, level);
    nodetree* n = &self->nt[off];
    int v = n->children[k];

    if (v < 0) {
      const char* n;
      Py_ssize_t i;

      v = -(v + 1);
      n = index_node(self, v);
      if (n == NULL)
        return -2;
      for (i = level; i < maxlevel; i++)
        if (getnybble(node, i) != nt_level(n, i))
          return -2;
      return v;
    }
    if (v == 0)
      return -2;
    off = v;
  }
  /* multiple matches against an ambiguous prefix */
  return -4;
}

static int nt_new(indexObject* self) {
  if (self->ntlength == self->ntcapacity) {
    if (self->ntcapacity >= SIZE_MAX / (sizeof(nodetree) * 2)) {
      PyErr_SetString(PyExc_MemoryError, "overflow in nt_new");
      return -1;
    }
    self->ntcapacity *= 2;
    self->nt = realloc(self->nt, self->ntcapacity * sizeof(nodetree));
    if (self->nt == NULL) {
      PyErr_SetString(PyExc_MemoryError, "out of memory");
      return -1;
    }
    memset(
        &self->nt[self->ntlength],
        0,
        sizeof(nodetree) * (self->ntcapacity - self->ntlength));
  }
  return self->ntlength++;
}

static int nt_insert(indexObject* self, const char* node, int rev) {
  int level = 0;
  int off = 0;

  while (level < 40) {
    int k = nt_level(node, level);
    nodetree* n;
    int v;

    n = &self->nt[off];
    v = n->children[k];

    if (v == 0) {
      n->children[k] = -rev - 1;
      return 0;
    }
    if (v < 0) {
      const char* oldnode = index_node(self, -(v + 1));
      int noff;

      if (!oldnode || !memcmp(oldnode, node, 20)) {
        n->children[k] = -rev - 1;
        return 0;
      }
      noff = nt_new(self);
      if (noff == -1)
        return -1;
      /* self->nt may have been changed by realloc */
      self->nt[off].children[k] = noff;
      off = noff;
      n = &self->nt[off];
      n->children[nt_level(oldnode, ++level)] = v;
      if (level > self->ntdepth)
        self->ntdepth = level;
      self->ntsplits += 1;
    } else {
      level += 1;
      off = v;
    }
  }

  return -1;
}

static int nt_init(indexObject* self) {
  if (self->nt == NULL) {
    if ((size_t)self->raw_length > SIZE_MAX / sizeof(nodetree)) {
      PyErr_SetString(PyExc_ValueError, "overflow in nt_init");
      return -1;
    }
    self->ntcapacity = self->raw_length < 4 ? 4 : (size_t)self->raw_length / 2;

    self->nt = calloc(self->ntcapacity, sizeof(nodetree));
    if (self->nt == NULL) {
      PyErr_NoMemory();
      return -1;
    }
    self->ntlength = 1;
    self->ntrev = (int)index_length(self) - 1;
    self->ntlookups = 1;
    self->ntmisses = 0;
    if (nt_insert(self, nullid, INT_MAX) == -1)
      return -1;
  }
  return 0;
}

/*
 * Return values:
 *
 *   -3: error (exception set)
 *   -2: not found (no exception set)
 * rest: valid rev
 */
static int
index_find_node(indexObject* self, const char* node, Py_ssize_t nodelen) {
  int rev;

  self->ntlookups++;
  rev = nt_find(self, node, nodelen, 0);
  if (rev >= -1)
    return rev;

  if (nt_init(self) == -1)
    return -3;

  /*
   * For the first handful of lookups, we scan the entire index,
   * and cache only the matching nodes. This optimizes for cases
   * like "hg tip", where only a few nodes are accessed.
   *
   * After that, we cache every node we visit, using a single
   * scan amortized over multiple lookups.  This gives the best
   * bulk performance, e.g. for "hg log".
   */
  if (self->ntmisses++ < 4) {
    for (rev = self->ntrev - 1; rev >= 0; rev--) {
      const char* n = index_node(self, rev);
      if (n == NULL)
        return -2;
      if (memcmp(node, n, nodelen > 20 ? 20 : nodelen) == 0) {
        if (nt_insert(self, n, rev) == -1)
          return -3;
        break;
      }
    }
  } else {
    for (rev = self->ntrev - 1; rev >= 0; rev--) {
      const char* n = index_node(self, rev);
      if (n == NULL) {
        self->ntrev = rev + 1;
        return -2;
      }
      if (nt_insert(self, n, rev) == -1) {
        self->ntrev = rev + 1;
        return -3;
      }
      if (memcmp(node, n, nodelen > 20 ? 20 : nodelen) == 0) {
        break;
      }
    }
    self->ntrev = rev;
  }

  if (rev >= 0)
    return rev;
  return -2;
}

static void raise_revlog_error(void) {
  PyObject *mod = NULL, *dict = NULL, *errclass = NULL;

  mod = PyImport_ImportModule("sapling.error");
  if (mod == NULL) {
    goto cleanup;
  }

  dict = PyModule_GetDict(mod);
  if (dict == NULL) {
    goto cleanup;
  }
  Py_INCREF(dict);

  errclass = PyDict_GetItemString(dict, "RevlogError");
  if (errclass == NULL) {
    PyErr_SetString(PyExc_SystemError, "could not find RevlogError");
    goto cleanup;
  }

  /* value of exception is ignored by callers */
  PyErr_SetString(errclass, "RevlogError");

cleanup:
  Py_XDECREF(dict);
  Py_XDECREF(mod);
}

static PyObject* index_getitem(indexObject* self, PyObject* value) {
  char* node;
  Py_ssize_t nodelen;
  int rev;

  if (PyInt_Check(value))
    return index_get(self, PyInt_AS_LONG(value));

  if (node_check(value, &node, &nodelen) == -1)
    return NULL;
  rev = index_find_node(self, node, nodelen);
  if (rev >= -1)
    return PyInt_FromLong(rev);
  if (rev == -2)
    raise_revlog_error();
  return NULL;
}

static int
nt_partialmatch(indexObject* self, const char* node, Py_ssize_t nodelen) {
  int rev;

  if (nt_init(self) == -1)
    return -3;

  if (self->ntrev > 0) {
    /* ensure that the radix tree is fully populated */
    for (rev = self->ntrev - 1; rev >= 0; rev--) {
      const char* n = index_node(self, rev);
      if (n == NULL)
        return -2;
      if (nt_insert(self, n, rev) == -1)
        return -3;
    }
    self->ntrev = rev;
  }

  return nt_find(self, node, nodelen, 1);
}

static PyObject* index_partialmatch(indexObject* self, PyObject* args) {
  const char* fullnode;
  Py_ssize_t nodelen;
  char* node;
  int rev, i;

  if (!PyArg_ParseTuple(args, "s#", &node, &nodelen))
    return NULL;

  if (nodelen < 4) {
    PyErr_SetString(PyExc_ValueError, "key too short");
    return NULL;
  }

  if (nodelen > 40) {
    PyErr_SetString(PyExc_ValueError, "key too long");
    return NULL;
  }

  for (i = 0; i < nodelen; i++)
    hexdigit(node, i);
  if (PyErr_Occurred()) {
    /* input contains non-hex characters */
    PyErr_Clear();
    Py_RETURN_NONE;
  }

  rev = nt_partialmatch(self, node, nodelen);

  switch (rev) {
    case -4:
      raise_revlog_error();
      return NULL;
    case -3:
      return NULL;
    case -2:
      Py_RETURN_NONE;
    case -1:
      return PyBytes_FromStringAndSize(nullid, 20);
  }

  fullnode = index_node(self, rev);
  if (fullnode == NULL) {
    PyErr_Format(PyExc_IndexError, "could not access rev %d", rev);
    return NULL;
  }
  return PyBytes_FromStringAndSize(fullnode, 20);
}

static PyObject* index_m_get(indexObject* self, PyObject* args) {
  Py_ssize_t nodelen;
  PyObject* val;
  char* node;
  int rev;

  if (!PyArg_ParseTuple(args, "O", &val))
    return NULL;
  if (node_check(val, &node, &nodelen) == -1)
    return NULL;
  rev = index_find_node(self, node, nodelen);
  if (rev == -3)
    return NULL;
  if (rev == -2)
    Py_RETURN_NONE;
  return PyInt_FromLong(rev);
}

static int index_contains(indexObject* self, PyObject* value) {
  char* node;
  Py_ssize_t nodelen;

  if (PyInt_Check(value)) {
    long rev = PyInt_AS_LONG(value);
    return rev >= -1 && rev < index_length(self);
  }

  if (node_check(value, &node, &nodelen) == -1)
    return -1;

  switch (index_find_node(self, node, nodelen)) {
    case -3:
      return -1;
    case -2:
      return 0;
    default:
      return 1;
  }
}

typedef uint64_t bitmask;

/*
 * Given a disjoint set of revs, return all candidates for the
 * greatest common ancestor. In revset notation, this is the set
 * "heads(::a and ::b and ...)"
 */
static PyObject*
find_gca_candidates(indexObject* self, const int* revs, int revcount) {
  const bitmask allseen = (1ull << revcount) - 1;
  const bitmask poison = 1ull << revcount;
  PyObject* gca = PyList_New(0);
  int i, v, interesting;
  int maxrev = -1;
  bitmask sp;
  bitmask* seen;

  if (gca == NULL)
    return PyErr_NoMemory();

  for (i = 0; i < revcount; i++) {
    if (revs[i] > maxrev)
      maxrev = revs[i];
  }

  seen = calloc(sizeof(*seen), maxrev + 1);
  if (seen == NULL) {
    Py_DECREF(gca);
    return PyErr_NoMemory();
  }

  for (i = 0; i < revcount; i++)
    seen[revs[i]] = 1ull << i;

  interesting = revcount;

  for (v = maxrev; v >= 0 && interesting; v--) {
    bitmask sv = seen[v];
    int parents[2];

    if (!sv)
      continue;

    if (sv < poison) {
      interesting -= 1;
      if (sv == allseen) {
        PyObject* obj = PyInt_FromLong(v);
        if (obj == NULL)
          goto bail;
        if (PyList_Append(gca, obj) == -1) {
          Py_DECREF(obj);
          goto bail;
        }
        sv |= poison;
        for (i = 0; i < revcount; i++) {
          if (revs[i] == v)
            goto done;
        }
      }
    }
    if (index_get_parents(self, v, parents, maxrev) < 0)
      goto bail;

    for (i = 0; i < 2; i++) {
      int p = parents[i];
      if (p == -1)
        continue;
      sp = seen[p];
      if (sv < poison) {
        if (sp == 0) {
          seen[p] = sv;
          interesting++;
        } else if (sp != sv)
          seen[p] |= sv;
      } else {
        if (sp && sp < poison)
          interesting--;
        seen[p] = sv;
      }
    }
  }

done:
  free(seen);
  return gca;
bail:
  free(seen);
  Py_XDECREF(gca);
  return NULL;
}

/*
 * Given a disjoint set of revs, return the subset with the longest
 * path to the root.
 */
static PyObject* find_deepest(indexObject* self, PyObject* revs) {
  const Py_ssize_t revcount = PyList_GET_SIZE(revs);
  static const Py_ssize_t capacity = 24;
  int *depth, *interesting = NULL;
  int i, j, v, ninteresting;
  PyObject *dict = NULL, *keys = NULL;
  long* seen = NULL;
  int maxrev = -1;
  long final;

  if (revcount > capacity) {
    PyErr_Format(
        PyExc_OverflowError,
        "bitset size (%ld) > capacity (%ld)",
        (long)revcount,
        (long)capacity);
    return NULL;
  }

  for (i = 0; i < revcount; i++) {
    int n = (int)PyInt_AsLong(PyList_GET_ITEM(revs, i));
    if (n > maxrev)
      maxrev = n;
  }

  depth = calloc(sizeof(*depth), maxrev + 1);
  if (depth == NULL)
    return PyErr_NoMemory();

  seen = calloc(sizeof(*seen), maxrev + 1);
  if (seen == NULL) {
    PyErr_NoMemory();
    goto bail;
  }

  interesting = calloc(sizeof(*interesting), 1 << revcount);
  if (interesting == NULL) {
    PyErr_NoMemory();
    goto bail;
  }

  if (PyList_Sort(revs) == -1)
    goto bail;

  for (i = 0; i < revcount; i++) {
    int n = (int)PyInt_AsLong(PyList_GET_ITEM(revs, i));
    long b = 1l << i;
    depth[n] = 1;
    seen[n] = b;
    interesting[b] = 1;
  }

  /* invariant: ninteresting is the number of non-zero entries in
   * interesting. */
  ninteresting = (int)revcount;

  for (v = maxrev; v >= 0 && ninteresting > 1; v--) {
    int dv = depth[v];
    int parents[2];
    long sv;

    if (dv == 0)
      continue;

    sv = seen[v];
    if (index_get_parents(self, v, parents, maxrev) < 0)
      goto bail;

    for (i = 0; i < 2; i++) {
      int p = parents[i];
      long sp;
      int dp;

      if (p == -1)
        continue;

      dp = depth[p];
      sp = seen[p];
      if (dp <= dv) {
        depth[p] = dv + 1;
        if (sp != sv) {
          interesting[sv] += 1;
          seen[p] = sv;
          if (sp) {
            interesting[sp] -= 1;
            if (interesting[sp] == 0)
              ninteresting -= 1;
          }
        }
      } else if (dv == dp - 1) {
        long nsp = sp | sv;
        if (nsp == sp)
          continue;
        seen[p] = nsp;
        interesting[sp] -= 1;
        if (interesting[sp] == 0)
          ninteresting -= 1;
        if (interesting[nsp] == 0)
          ninteresting += 1;
        interesting[nsp] += 1;
      }
    }
    interesting[sv] -= 1;
    if (interesting[sv] == 0)
      ninteresting -= 1;
  }

  final = 0;
  j = ninteresting;
  for (i = 0; i < (int)(2 << revcount) && j > 0; i++) {
    if (interesting[i] == 0)
      continue;
    final |= i;
    j -= 1;
  }
  if (final == 0) {
    keys = PyList_New(0);
    goto bail;
  }

  dict = PyDict_New();
  if (dict == NULL)
    goto bail;

  for (i = 0; i < revcount; i++) {
    PyObject* key;

    if ((final & (1 << i)) == 0)
      continue;

    key = PyList_GET_ITEM(revs, i);
    Py_INCREF(key);
    Py_INCREF(Py_None);
    if (PyDict_SetItem(dict, key, Py_None) == -1) {
      Py_DECREF(key);
      Py_DECREF(Py_None);
      goto bail;
    }
  }

  keys = PyDict_Keys(dict);

bail:
  free(depth);
  free(seen);
  free(interesting);
  Py_XDECREF(dict);

  return keys;
}

/*
 * Given a (possibly overlapping) set of revs, return all the
 * common ancestors heads: heads(::args[0] and ::a[1] and ...)
 */
static PyObject* index_commonancestorsheads(indexObject* self, PyObject* args) {
  PyObject* ret = NULL;
  Py_ssize_t argcount, i, len;
  bitmask repeat = 0;
  int revcount = 0;
  int* revs;

  argcount = PySequence_Length(args);
  revs = PyMem_Malloc(argcount * sizeof(*revs));
  if (argcount > 0 && revs == NULL)
    return PyErr_NoMemory();
  len = index_length(self) - 1;

  for (i = 0; i < argcount; i++) {
    static const int capacity = 24;
    PyObject* obj = PySequence_GetItem(args, i);
    bitmask x;
    long val;

    if (!PyInt_Check(obj)) {
      PyErr_SetString(PyExc_TypeError, "arguments must all be ints");
      Py_DECREF(obj);
      goto bail;
    }
    val = PyInt_AsLong(obj);
    Py_DECREF(obj);
    if (val == -1) {
      ret = PyList_New(0);
      goto done;
    }
    if (val < 0 || val >= len) {
      PyErr_SetString(PyExc_IndexError, "index out of range");
      goto bail;
    }
    /* this cheesy bloom filter lets us avoid some more
     * expensive duplicate checks in the common set-is-disjoint
     * case */
    x = 1ull << (val & 0x3f);
    if (repeat & x) {
      int k;
      for (k = 0; k < revcount; k++) {
        if (val == revs[k])
          goto duplicate;
      }
    } else
      repeat |= x;
    if (revcount >= capacity) {
      PyErr_Format(
          PyExc_OverflowError,
          "bitset size (%d) > capacity (%d)",
          revcount,
          capacity);
      goto bail;
    }
    revs[revcount++] = (int)val;
  duplicate:;
  }

  if (revcount == 0) {
    ret = PyList_New(0);
    goto done;
  }
  if (revcount == 1) {
    PyObject* obj;
    ret = PyList_New(1);
    if (ret == NULL)
      goto bail;
    obj = PyInt_FromLong(revs[0]);
    if (obj == NULL)
      goto bail;
    PyList_SET_ITEM(ret, 0, obj);
    goto done;
  }

  ret = find_gca_candidates(self, revs, revcount);
  if (ret == NULL)
    goto bail;

done:
  PyMem_Free(revs);
  return ret;

bail:
  PyMem_Free(revs);
  Py_XDECREF(ret);
  return NULL;
}

/*
 * Given a (possibly overlapping) set of revs, return the greatest
 * common ancestors: those with the longest path to the root.
 */
static PyObject* index_ancestors(indexObject* self, PyObject* args) {
  PyObject* ret;
  PyObject* gca = index_commonancestorsheads(self, args);
  if (gca == NULL)
    return NULL;

  if (PyList_GET_SIZE(gca) <= 1) {
    return gca;
  }

  ret = find_deepest(self, gca);
  Py_DECREF(gca);
  return ret;
}

/*
 * Invalidate any trie entries introduced by added revs.
 */
static void nt_invalidate_added(indexObject* self, Py_ssize_t start) {
  Py_ssize_t i, len = PyList_GET_SIZE(self->added);

  for (i = start; i < len; i++) {
    PyObject* tuple = PyList_GET_ITEM(self->added, i);
    PyObject* node = PyTuple_GET_ITEM(tuple, 7);

    nt_insert(self, PyBytes_AS_STRING(node), -1);
  }

  if (start == 0)
    Py_CLEAR(self->added);
}

/*
 * Delete a numeric range of revs, which must be at the end of the
 * range, but exclude the sentinel nullid entry.
 */
static int index_slice_del(indexObject* self, PyObject* item) {
  Py_ssize_t start, stop, step, slicelength;
  Py_ssize_t length = index_length(self);
  int ret = 0;

/* Argument changed from PySliceObject* to PyObject* in Python 3. */
#ifdef IS_PY3K
  if (PySlice_GetIndicesEx(item, length, &start, &stop, &step, &slicelength) <
      0)
#else
  if (PySlice_GetIndicesEx(
          (PySliceObject*)item, length, &start, &stop, &step, &slicelength) < 0)
#endif
    return -1;

  if (slicelength <= 0)
    return 0;

  if ((step < 0 && start < stop) || (step > 0 && start > stop))
    stop = start;

  if (step < 0) {
    stop = start + 1;
    start = stop + step * (slicelength - 1) - 1;
    step = -step;
  }

  if (step != 1) {
    PyErr_SetString(
        PyExc_ValueError, "revlog index delete requires step size of 1");
    return -1;
  }

  if (stop != length - 1) {
    PyErr_SetString(
        PyExc_IndexError, "revlog index deletion indices are invalid");
    return -1;
  }

  if (start < self->length - 1) {
    if (self->nt) {
      Py_ssize_t i;

      for (i = start + 1; i < self->length - 1; i++) {
        const char* node = index_node(self, i);

        if (node)
          nt_insert(self, node, -1);
      }
      if (self->added)
        nt_invalidate_added(self, 0);
      if (self->ntrev > start)
        self->ntrev = (int)start;
    }
    self->length = start + 1;
    if (start < self->raw_length) {
      if (self->cache) {
        Py_ssize_t i;
        for (i = start; i < self->raw_length; i++)
          Py_CLEAR(self->cache[i]);
      }
      self->raw_length = start;
    }
    goto done;
  }

  if (self->nt) {
    nt_invalidate_added(self, start - self->length + 1);
    if (self->ntrev > start)
      self->ntrev = (int)start;
  }
  if (self->added)
    ret = PyList_SetSlice(
        self->added,
        start - self->length + 1,
        PyList_GET_SIZE(self->added),
        NULL);
done:
  Py_CLEAR(self->headrevs);
  return ret;
}

/*
 * Supported ops:
 *
 * slice deletion
 * string assignment (extend node->rev mapping)
 * string deletion (shrink node->rev mapping)
 */
static int
index_assign_subscript(indexObject* self, PyObject* item, PyObject* value) {
  char* node;
  Py_ssize_t nodelen;
  long rev;

  if (PySlice_Check(item) && value == NULL)
    return index_slice_del(self, item);

  if (node_check(item, &node, &nodelen) == -1)
    return -1;

  if (value == NULL)
    return self->nt ? nt_insert(self, node, -1) : 0;
  rev = PyInt_AsLong(value);
  if (rev > INT_MAX || rev < 0) {
    if (!PyErr_Occurred())
      PyErr_SetString(PyExc_ValueError, "rev out of range");
    return -1;
  }

  if (nt_init(self) == -1)
    return -1;
  return nt_insert(self, node, (int)rev);
}

/*
 * Find all RevlogNG entries in an index that has inline data. Update
 * the optional "offsets" table with those entries.
 */
static Py_ssize_t inline_scan(indexObject* self, const char** offsets) {
  const char* data = (const char*)self->buf.buf;
  Py_ssize_t pos = 0;
  Py_ssize_t end = self->buf.len;
  long incr = v1_hdrsize;
  Py_ssize_t len = 0;

  while (pos + v1_hdrsize <= end && pos >= 0) {
    uint32_t comp_len;
    /* 3rd element of header is length of compressed inline data */
    comp_len = getbe32(data + pos + 8);
    incr = v1_hdrsize + comp_len;
    if (offsets)
      offsets[len] = data + pos;
    len++;
    pos += incr;
  }

  if (pos != end) {
    if (!PyErr_Occurred())
      PyErr_SetString(PyExc_ValueError, "corrupt index file");
    return -1;
  }

  return len;
}

static int index_init(indexObject* self, PyObject* args) {
  PyObject *data_obj, *inlined_obj;
  Py_ssize_t size;

  /* Initialize before argument-checking to avoid index_dealloc() crash. */
  self->raw_length = 0;
  self->added = NULL;
  self->cache = NULL;
  self->data = NULL;
  memset(&self->buf, 0, sizeof(self->buf));
  self->headrevs = NULL;
  Py_INCREF(Py_None);
  self->nt = NULL;
  self->offsets = NULL;

  if (!PyArg_ParseTuple(args, "OO", &data_obj, &inlined_obj))
    return -1;
  if (!PyObject_CheckBuffer(data_obj)) {
    PyErr_SetString(PyExc_TypeError, "data does not support buffer interface");
    return -1;
  }

  if (PyObject_GetBuffer(data_obj, &self->buf, PyBUF_SIMPLE) == -1)
    return -1;
  size = self->buf.len;

  self->inlined = inlined_obj && PyObject_IsTrue(inlined_obj);
  self->data = data_obj;

  self->ntlength = self->ntcapacity = 0;
  self->ntdepth = self->ntsplits = 0;
  self->ntlookups = self->ntmisses = 0;
  self->ntrev = -1;
  Py_INCREF(self->data);

  if (self->inlined) {
    Py_ssize_t len = inline_scan(self, NULL);
    if (len == -1)
      goto bail;
    self->raw_length = len;
    self->length = len + 1;
  } else {
    if (size % v1_hdrsize) {
      PyErr_SetString(PyExc_ValueError, "corrupt index file");
      goto bail;
    }
    self->raw_length = size / v1_hdrsize;
    self->length = self->raw_length + 1;
  }

  return 0;
bail:
  return -1;
}

static PyObject* index_nodemap(indexObject* self) {
  Py_INCREF(self);
  return (PyObject*)self;
}

static void index_dealloc(indexObject* self) {
  _index_clearcaches(self);
  if (self->buf.buf) {
    PyBuffer_Release(&self->buf);
    memset(&self->buf, 0, sizeof(self->buf));
  }
  Py_XDECREF(self->data);
  Py_XDECREF(self->added);
  PyObject_Del(self);
}

static PySequenceMethods index_sequence_methods = {
    (lenfunc)index_length, /* sq_length */
    0, /* sq_concat */
    0, /* sq_repeat */
    (ssizeargfunc)index_get, /* sq_item */
    0, /* sq_slice */
    0, /* sq_ass_item */
    0, /* sq_ass_slice */
    (objobjproc)index_contains, /* sq_contains */
};

static PyMappingMethods index_mapping_methods = {
    (lenfunc)index_length, /* mp_length */
    (binaryfunc)index_getitem, /* mp_subscript */
    (objobjargproc)index_assign_subscript, /* mp_ass_subscript */
};

static PyMethodDef index_methods[] = {
    {"ancestors",
     (PyCFunction)index_ancestors,
     METH_VARARGS,
     "return the gca set of the given revs"},
    {"commonancestorsheads",
     (PyCFunction)index_commonancestorsheads,
     METH_VARARGS,
     "return the heads of the common ancestors of the given revs"},
    {"clearcaches",
     (PyCFunction)index_clearcaches,
     METH_NOARGS,
     "clear the index caches"},
    {"get", (PyCFunction)index_m_get, METH_VARARGS, "get an index entry"},
    {"computephasesmapsets",
     (PyCFunction)compute_phases_map_sets,
     METH_VARARGS,
     "compute phases"},
    {"reachableroots2",
     (PyCFunction)reachableroots2,
     METH_VARARGS,
     "reachableroots"},
    {"headrevs",
     (PyCFunction)index_headrevs,
     METH_VARARGS,
     "get head revisions"},
    {"deltachain",
     (PyCFunction)index_deltachain,
     METH_VARARGS,
     "determine revisions with deltas to reconstruct fulltext"},
    {"insert",
     (PyCFunction)index_insert,
     METH_VARARGS,
     "insert an index entry"},
    {"partialmatch",
     (PyCFunction)index_partialmatch,
     METH_VARARGS,
     "match a potentially ambiguous node ID"},
    {"stats", (PyCFunction)index_stats, METH_NOARGS, "stats for the index"},
    {NULL} /* Sentinel */
};

static PyGetSetDef index_getset[] = {
    {"nodemap", (getter)index_nodemap, NULL, "nodemap", NULL},
    {NULL} /* Sentinel */
};

static PyTypeObject indexType = {
    PyVarObject_HEAD_INIT(NULL, 0) /* header */
    "parsers.index", /* tp_name */
    sizeof(indexObject), /* tp_basicsize */
    0, /* tp_itemsize */
    (destructor)index_dealloc, /* tp_dealloc */
    0, /* tp_print */
    0, /* tp_getattr */
    0, /* tp_setattr */
    0, /* tp_compare */
    0, /* tp_repr */
    0, /* tp_as_number */
    &index_sequence_methods, /* tp_as_sequence */
    &index_mapping_methods, /* tp_as_mapping */
    0, /* tp_hash */
    0, /* tp_call */
    0, /* tp_str */
    0, /* tp_getattro */
    0, /* tp_setattro */
    0, /* tp_as_buffer */
    Py_TPFLAGS_DEFAULT, /* tp_flags */
    "revlog index", /* tp_doc */
    0, /* tp_traverse */
    0, /* tp_clear */
    0, /* tp_richcompare */
    0, /* tp_weaklistoffset */
    0, /* tp_iter */
    0, /* tp_iternext */
    index_methods, /* tp_methods */
    0, /* tp_members */
    index_getset, /* tp_getset */
    0, /* tp_base */
    0, /* tp_dict */
    0, /* tp_descr_get */
    0, /* tp_descr_set */
    0, /* tp_dictoffset */
    (initproc)index_init, /* tp_init */
    0, /* tp_alloc */
};

/*
 * returns a tuple of the form (index, index, cache) with elements as
 * follows:
 *
 * index: an index object that lazily parses RevlogNG records
 * cache: if data is inlined, a tuple (0, index_file_content), else None
 *        index_file_content could be a string, or a buffer
 *
 * added complications are for backwards compatibility
 */
PyObject* parse_index2(PyObject* self, PyObject* args) {
  PyObject *tuple = NULL, *cache = NULL;
  indexObject* idx;
  int ret;

  idx = PyObject_New(indexObject, &indexType);
  if (idx == NULL)
    goto bail;

  ret = index_init(idx, args);
  if (ret == -1)
    goto bail;

  if (idx->inlined) {
    cache = Py_BuildValue("iO", 0, idx->data);
    if (cache == NULL)
      goto bail;
  } else {
    cache = Py_None;
    Py_INCREF(cache);
  }

  tuple = Py_BuildValue("NN", idx, cache);
  if (!tuple)
    goto bail;
  return tuple;

bail:
  Py_XDECREF(idx);
  Py_XDECREF(cache);
  Py_XDECREF(tuple);
  return NULL;
}

void revlog_module_init(PyObject* mod) {
  indexType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&indexType) < 0)
    return;
  Py_INCREF(&indexType);
  PyModule_AddObject(mod, "index", (PyObject*)&indexType);

#ifdef IS_PY3K
  nullentry = Py_BuildValue(
      "iiiiiiiy#", 0, 0, 0, -1, -1, -1, -1, nullid, (Py_ssize_t)20);
#else
  nullentry = Py_BuildValue(
      "iiiiiiis#", 0, 0, 0, -1, -1, -1, -1, nullid, (Py_ssize_t)20);
#endif
  if (nullentry)
    PyObject_GC_UnTrack(nullentry);
}
