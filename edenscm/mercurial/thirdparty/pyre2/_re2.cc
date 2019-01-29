/*
 * Copyright (c) 2010, David Reiss and Facebook, Inc. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * * Redistributions of source code must retain the above copyright
 *   notice, this list of conditions and the following disclaimer.
 * * Redistributions in binary form must reproduce the above copyright
 *   notice, this list of conditions and the following disclaimer in the
 *   documentation and/or other materials provided with the distribution.
 * * Neither the name of Facebook nor the names of its contributors
 *   may be used to endorse or promote products derived from this software
 *   without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
 * HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include <cstddef>

#include <string>
#include <new>
using std::nothrow;

#include <re2/re2.h>
#include <re2/set.h>
using re2::RE2;
using re2::StringPiece;


typedef struct _RegexpObject2 {
  PyObject_HEAD
  // __dict__.  Simpler than implementing getattr and possibly faster.
  PyObject* attr_dict;
  RE2* re2_obj;
} RegexpObject2;

typedef struct _MatchObject2 {
  PyObject_HEAD
  // __dict__.  Simpler than implementing getattr and possibly faster.
  PyObject* attr_dict;
  // Cache of __dict__["re"] and __dict__["string", which are used for group()
  // calls. These fields do *not* own their own references.  They piggyback on
  // the references in attr_dict.
  PyObject* re;
  PyObject* string;
  // There are several possible approaches to storing the matched groups:
  // 1. Fully materialize the groups tuple at match time.
  // 2. Cache allocated PyBytes objects when groups are requested.
  // 3. Always allocate new PyBytess on demand.
  // I've chosen to go with #3.  It's the simplest, and I'm pretty sure it's
  // optimal in all cases where no group is fetched more than once.
  StringPiece* groups;
} MatchObject2;

typedef struct _RegexpSetObject2 {
  PyObject_HEAD
  // True iff re2_set_obj has been compiled.
  bool compiled;
  RE2::Set* re2_set_obj;
} RegexpSetObject2;


// Forward declarations of methods, creators, and destructors.
static void regexp_dealloc(RegexpObject2* self);
static PyObject* create_regexp(PyObject* self, PyObject* pattern, PyObject* error_class);
static PyObject* regexp_search(RegexpObject2* self, PyObject* args, PyObject* kwds);
static PyObject* regexp_match(RegexpObject2* self, PyObject* args, PyObject* kwds);
static PyObject* regexp_fullmatch(RegexpObject2* self, PyObject* args, PyObject* kwds);
static PyObject* regexp_test_search(RegexpObject2* self, PyObject* args, PyObject* kwds);
static PyObject* regexp_test_match(RegexpObject2* self, PyObject* args, PyObject* kwds);
static PyObject* regexp_test_fullmatch(RegexpObject2* self, PyObject* args, PyObject* kwds);
static void match_dealloc(MatchObject2* self);
static PyObject* create_match(PyObject* re, PyObject* string, long pos, long endpos, StringPiece* groups);
static PyObject* match_group(MatchObject2* self, PyObject* args);
static PyObject* match_groups(MatchObject2* self, PyObject* args, PyObject* kwds);
static PyObject* match_groupdict(MatchObject2* self, PyObject* args, PyObject* kwds);
static PyObject* match_start(MatchObject2* self, PyObject* args);
static PyObject* match_end(MatchObject2* self, PyObject* args);
static PyObject* match_span(MatchObject2* self, PyObject* args);
static void regexp_set_dealloc(RegexpSetObject2* self);
static PyObject* regexp_set_new(PyTypeObject* type, PyObject* args, PyObject* kwds);
static PyObject* regexp_set_add(RegexpSetObject2* self, PyObject* pattern);
static PyObject* regexp_set_compile(RegexpSetObject2* self);
static PyObject* regexp_set_match(RegexpSetObject2* self, PyObject* text);


static PyMethodDef regexp_methods[] = {
  {"search", (PyCFunction)regexp_search, METH_VARARGS | METH_KEYWORDS,
    "search(string[, pos[, endpos]]) --> match object or None.\n"
    "    Scan through string looking for a match, and return a corresponding\n"
    "    MatchObject instance. Return None if no position in the string matches."
  },
  {"match", (PyCFunction)regexp_match, METH_VARARGS | METH_KEYWORDS,
    "match(string[, pos[, endpos]]) --> match object or None.\n"
    "    Matches zero or more characters at the beginning of the string"
  },
  {"fullmatch", (PyCFunction)regexp_fullmatch, METH_VARARGS | METH_KEYWORDS,
    "fullmatch(string[, pos[, endpos]]) --> match object or None.\n"
    "    Matches the entire string"
  },
  {"test_search", (PyCFunction)regexp_test_search, METH_VARARGS | METH_KEYWORDS,
    "test_search(string[, pos[, endpos]]) --> bool.\n"
    "    Like 'search', but only returns whether a match was found."
  },
  {"test_match", (PyCFunction)regexp_test_match, METH_VARARGS | METH_KEYWORDS,
    "test_match(string[, pos[, endpos]]) --> match object or None.\n"
    "    Like 'match', but only returns whether a match was found."
  },
  {"test_fullmatch", (PyCFunction)regexp_test_fullmatch, METH_VARARGS | METH_KEYWORDS,
    "test_fullmatch(string[, pos[, endpos]]) --> match object or None.\n"
    "    Like 'fullmatch', but only returns whether a match was found."
  },
  {NULL}  /* Sentinel */
};

static PyMethodDef match_methods[] = {
  {"group", (PyCFunction)match_group, METH_VARARGS,
    NULL
  },
  {"groups", (PyCFunction)match_groups, METH_VARARGS | METH_KEYWORDS,
    NULL
  },
  {"groupdict", (PyCFunction)match_groupdict, METH_VARARGS | METH_KEYWORDS,
    NULL
  },
  {"start", (PyCFunction)match_start, METH_VARARGS,
    NULL
  },
  {"end", (PyCFunction)match_end, METH_VARARGS,
    NULL
  },
  {"span", (PyCFunction)match_span, METH_VARARGS,
    NULL
  },
  {NULL}  /* Sentinel */
};

static PyMethodDef regexp_set_methods[] = {
  {"add", (PyCFunction)regexp_set_add, METH_O,
    "add(pattern) --> index or raises an exception.\n"
    "    Add a pattern to the set, raising if the pattern doesn't parse."
  },
  {"compile", (PyCFunction)regexp_set_compile, METH_NOARGS,
    "compile() --> None or raises an exception.\n"
    "    Compile the set to prepare it for matching."
  },
  {"match", (PyCFunction)regexp_set_match, METH_O,
    "match(text) --> list\n"
    "    Match text against the set, returning the indexes of the added patterns."
  },
  {NULL}  /* Sentinel */
};


// Simple method to block setattr.
static int
_no_setattr(PyObject* obj, PyObject* name, PyObject* v) {
  (void)name;
  (void)v;
	PyErr_Format(PyExc_AttributeError,
      "'%s' object attributes are read-only",
      obj->ob_type->tp_name);
  return -1;
}

#if PY_MAJOR_VERSION >= 3
#define REGEX_OBJECT_TYPE &PyUnicode_Type
#else
#define REGEX_OBJECT_TYPE &PyString_Type
#endif

static PyTypeObject Regexp_Type2 = {
  PyObject_HEAD_INIT(NULL)
#if PY_MAJOR_VERSION < 3
  0,                           /*ob_size*/
#endif
  "_re2.RE2_Regexp",           /*tp_name*/
  sizeof(RegexpObject2),       /*tp_basicsize*/
  0,                           /*tp_itemsize*/
  (destructor)regexp_dealloc,  /*tp_dealloc*/
  0,                           /*tp_print*/
  0,                           /*tp_getattr*/
  0,                           /*tp_setattr*/
  0,                           /*tp_compare*/
  0,                           /*tp_repr*/
  0,                           /*tp_as_number*/
  0,                           /*tp_as_sequence*/
  0,                           /*tp_as_mapping*/
  0,                           /*tp_hash*/
  0,                           /*tp_call*/
  0,                           /*tp_str*/
  0,                           /*tp_getattro*/
  _no_setattr,                 /*tp_setattro*/
  0,                           /*tp_as_buffer*/
  Py_TPFLAGS_DEFAULT,          /*tp_flags*/
  "RE2 regexp objects",        /*tp_doc*/
  0,                           /*tp_traverse*/
  0,                           /*tp_clear*/
  0,                           /*tp_richcompare*/
  0,                           /*tp_weaklistoffset*/
  0,                           /*tp_iter*/
  0,                           /*tp_iternext*/
  regexp_methods,              /*tp_methods*/
  0,                           /*tp_members*/
  0,                           /*tp_getset*/
  0,                           /*tp_base*/
  0,                           /*tp_dict*/
  0,                           /*tp_descr_get*/
  0,                           /*tp_descr_set*/
  offsetof(RegexpObject2, attr_dict),  /*tp_dictoffset*/
  0,                           /*tp_init*/
  0,                           /*tp_alloc*/
  0,                           /*tp_new*/
};

static PyTypeObject Match_Type2 = {
  PyObject_HEAD_INIT(NULL)
#if PY_MAJOR_VERSION < 3
  0,                           /*ob_size*/
#endif
  "_re2.RE2_Match",            /*tp_name*/
  sizeof(MatchObject2),        /*tp_basicsize*/
  0,                           /*tp_itemsize*/
  (destructor)match_dealloc,   /*tp_dealloc*/
  0,                           /*tp_print*/
  0,                           /*tp_getattr*/
  0,                           /*tp_setattr*/
  0,                           /*tp_compare*/
  0,                           /*tp_repr*/
  0,                           /*tp_as_number*/
  0,                           /*tp_as_sequence*/
  0,                           /*tp_as_mapping*/
  0,                           /*tp_hash*/
  0,                           /*tp_call*/
  0,                           /*tp_str*/
  0,                           /*tp_getattro*/
  _no_setattr,                 /*tp_setattro*/
  0,                           /*tp_as_buffer*/
  Py_TPFLAGS_DEFAULT,          /*tp_flags*/
  "RE2 match objects",         /*tp_doc*/
  0,                           /*tp_traverse*/
  0,                           /*tp_clear*/
  0,                           /*tp_richcompare*/
  0,                           /*tp_weaklistoffset*/
  0,                           /*tp_iter*/
  0,                           /*tp_iternext*/
  match_methods,               /*tp_methods*/
  0,                           /*tp_members*/
  0,                           /*tp_getset*/
  0,                           /*tp_base*/
  0,                           /*tp_dict*/
  0,                           /*tp_descr_get*/
  0,                           /*tp_descr_set*/
  offsetof(MatchObject2, attr_dict),  /*tp_dictoffset*/
  0,                           /*tp_init*/
  0,                           /*tp_alloc*/
  0,                           /*tp_new*/
};

static PyTypeObject RegexpSet_Type2 = {
  PyObject_HEAD_INIT(NULL)
#if PY_MAJOR_VERSION < 3
  0,                               /*ob_size*/
#endif
  "_re2.RE2_Set",                  /*tp_name*/
  sizeof(RegexpSetObject2),        /*tp_basicsize*/
  0,                               /*tp_itemsize*/
  (destructor)regexp_set_dealloc,  /*tp_dealloc*/
  0,                               /*tp_print*/
  0,                               /*tp_getattr*/
  0,                               /*tp_setattr*/
  0,                               /*tp_compare*/
  0,                               /*tp_repr*/
  0,                               /*tp_as_number*/
  0,                               /*tp_as_sequence*/
  0,                               /*tp_as_mapping*/
  0,                               /*tp_hash*/
  0,                               /*tp_call*/
  0,                               /*tp_str*/
  0,                               /*tp_getattro*/
  _no_setattr,                     /*tp_setattro*/
  0,                               /*tp_as_buffer*/
  Py_TPFLAGS_DEFAULT,              /*tp_flags*/
  "RE2 regexp set objects",        /*tp_doc*/
  0,                               /*tp_traverse*/
  0,                               /*tp_clear*/
  0,                               /*tp_richcompare*/
  0,                               /*tp_weaklistoffset*/
  0,                               /*tp_iter*/
  0,                               /*tp_iternext*/
  regexp_set_methods,              /*tp_methods*/
  0,                               /*tp_members*/
  0,                               /*tp_getset*/
  0,                               /*tp_base*/
  0,                               /*tp_dict*/
  0,                               /*tp_descr_get*/
  0,                               /*tp_descr_set*/
  0,                               /*tp_dictoffset*/
  0,                               /*tp_init*/
  0,                               /*tp_alloc*/
  regexp_set_new,                  /*tp_new*/
};


static void
regexp_dealloc(RegexpObject2* self)
{
  delete self->re2_obj;
  Py_XDECREF(self->attr_dict);
  PyObject_Del(self);
}

static PyObject*
create_regexp(PyObject* self, PyObject* pattern, PyObject* error_class)
{
  RegexpObject2* regexp = PyObject_New(RegexpObject2, &Regexp_Type2);
  if (regexp == NULL) {
    return NULL;
  }
  regexp->re2_obj = NULL;
  regexp->attr_dict = NULL;

  Py_ssize_t len_pattern;
#if PY_MAJOR_VERSION >= 3
  const char* raw_pattern = PyUnicode_AsUTF8AndSize(pattern, &len_pattern);
#else
  const char* raw_pattern = PyString_AS_STRING(pattern);
  len_pattern = PyString_GET_SIZE(pattern);
#endif

  RE2::Options options;
  options.set_log_errors(false);

  regexp->re2_obj = new(nothrow) RE2(StringPiece(raw_pattern, (int)len_pattern), options);

  if (regexp->re2_obj == NULL) {
    PyErr_NoMemory();
    Py_DECREF(regexp);
    return NULL;
  }

  if (!regexp->re2_obj->ok()) {
    long code = (long)regexp->re2_obj->error_code();
    const std::string& msg = regexp->re2_obj->error();
    PyObject* value = Py_BuildValue("ls#", code, msg.data(), msg.length());
    if (value == NULL) {
      Py_DECREF(regexp);
      return NULL;
    }
    PyErr_SetObject(error_class, value);
    Py_DECREF(regexp);
    return NULL;
  }

  PyObject* groupindex = PyDict_New();
  if (groupindex == NULL) {
    Py_DECREF(regexp);
    return NULL;
  }

  // Build up the attr_dict early so regexp can take ownership of our reference
  // to groupindex.
  regexp->attr_dict = Py_BuildValue("{sisNsO}",
      "groups", regexp->re2_obj->NumberOfCapturingGroups(),
      "groupindex", groupindex,
      "pattern", pattern);
  if (regexp->attr_dict == NULL) {
    Py_DECREF(regexp);
    return NULL;
  }

  const std::map<std::string, int>& name_map = regexp->re2_obj->NamedCapturingGroups();
  for (std::map<std::string, int>::const_iterator it = name_map.begin(); it != name_map.end(); ++it) {
    // This used to return an int() on Py2, but now returns a long() to be
    // consistent across Py3 and Py2.
    PyObject* index = PyLong_FromLong(it->second);
    if (index == NULL) {
      Py_DECREF(regexp);
      return NULL;
    }
    int res = PyDict_SetItemString(groupindex, it->first.c_str(), index);
    Py_DECREF(index);
    if (res < 0) {
      Py_DECREF(regexp);
      return NULL;
    }
  }

  return (PyObject*)regexp;
}

static PyObject*
_do_search(RegexpObject2* self, PyObject* args, PyObject* kwds, RE2::Anchor anchor, bool return_match)
{
  PyObject* string;
  long pos = 0;
  long endpos = LONG_MAX;

  static const char* kwlist[] = {
    "string",
    "pos",
    "endpos",
    NULL};

  // Using O instead of s# here, because we want to stash the original
  // PyObject* in the match object on a successful match.
  if (!PyArg_ParseTupleAndKeywords(args, kwds, "O|ll", (char**)kwlist,
        &string,
        &pos, &endpos)) {
    return NULL;
  }

  const char *subject;
  Py_ssize_t slen;
#if PY_MAJOR_VERSION >= 3
  if (PyUnicode_Check(string)) {
    subject = PyUnicode_AsUTF8AndSize(string, &slen);
  } else if (PyBytes_Check(string)) {
    subject = PyBytes_AS_STRING(string);
    slen = PyBytes_GET_SIZE(string);
  } else {
    PyErr_SetString(PyExc_TypeError, "can only operate on unicode or bytes");
    return NULL;
  }
#else
  subject = PyString_AsString(string);
  if (subject == NULL) {
    return NULL;
  }
  slen = PyString_GET_SIZE(string);
#endif
  if (pos < 0) pos = 0;
  if (pos > slen) pos = slen;
  if (endpos < pos) endpos = pos;
  if (endpos > slen) endpos = slen;

  // Don't bother allocating these if we are just doing a test.
  int n_groups = 0;
  StringPiece* groups = NULL;
  if (return_match) {
    n_groups = self->re2_obj->NumberOfCapturingGroups() + 1;
    groups = new(nothrow) StringPiece[n_groups];

    if (groups == NULL) {
      PyErr_NoMemory();
      return NULL;
    }
  }

  bool matched = self->re2_obj->Match(
      StringPiece(subject, (int)slen),
      (int)pos,
      (int)endpos,
      anchor,
      groups,
      n_groups);

  if (!return_match) {
    if (matched) {
      Py_RETURN_TRUE;
    }
    Py_RETURN_FALSE;
  }

  if (!matched) {
    delete[] groups;
    Py_RETURN_NONE;
  }

  // create_match is going to Py_BuildValue the pos and endpos into
  // PyObjects.  We could optimize the case where pos and/or endpos were
  // explicitly passed in by forwarding the existing PyObjects.
  // That requires much more intricate code, though.
  return create_match((PyObject*)self, string, pos, endpos, groups);
}

static PyObject*
regexp_search(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::UNANCHORED, true);
}

static PyObject*
regexp_match(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::ANCHOR_START, true);
}

static PyObject*
regexp_fullmatch(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::ANCHOR_BOTH, true);
}

static PyObject*
regexp_test_search(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::UNANCHORED, false);
}

static PyObject*
regexp_test_match(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::ANCHOR_START, false);
}

static PyObject*
regexp_test_fullmatch(RegexpObject2* self, PyObject* args, PyObject* kwds)
{
  return _do_search(self, args, kwds, RE2::ANCHOR_BOTH, false);
}


static void
match_dealloc(MatchObject2* self)
{
  delete[] self->groups;
  Py_XDECREF(self->attr_dict);
  PyObject_Del(self);
}

static PyObject*
create_match(PyObject* re, PyObject* string,
    long pos, long endpos,
    StringPiece* groups)
{
  MatchObject2* match = PyObject_New(MatchObject2, &Match_Type2);
  if (match == NULL) {
    delete[] groups;
    return NULL;
  }
  match->attr_dict = NULL;
  match->groups = groups;
  match->re = re;
  match->string = string;

  match->attr_dict = Py_BuildValue("{sOsOslsl}",
      "re", re,
      "string", string,
      "pos", pos,
      "endpos", endpos);
  if (match->attr_dict == NULL) {
    Py_DECREF(match);
    return NULL;
  }

  return (PyObject*)match;
}

/**
 * Attempt to convert an untrusted group index (PyObject* group) into
 * a trusted one (*idx_p).  Return false on failure (exception).
 */
static bool
_group_idx(MatchObject2* self, PyObject* group, long* idx_p)
{
  if (group == NULL) {
    return false;
  }
  PyErr_Clear(); // Is this necessary?
  long idx = PyLong_AsLong(group);
  if (idx == -1 && PyErr_Occurred() != NULL) {
    return false;
  }
  // TODO: Consider caching NumberOfCapturingGroups.
  if (idx < 0 || idx > ((RegexpObject2*)self->re)->re2_obj->NumberOfCapturingGroups()) {
    PyErr_SetString(PyExc_IndexError, "no such group");
    return false;
  }
  *idx_p = idx;
  return true;
}

/**
 * Extract the start and end indexes of a pre-checked group number.
 * Sets both to -1 if it did not participate in the match.
 */
static bool
_group_span(MatchObject2* self, long idx, Py_ssize_t* o_start, Py_ssize_t* o_end)
{
  // "idx" is expected to be verified.
  StringPiece& piece = self->groups[idx];
  if (piece.data() == NULL) {
    *o_start = -1;
    *o_end = -1;
    return false;
  }
  Py_ssize_t start;
#if PY_MAJOR_VERSION >= 3
  if (PyBytes_Check(self->string)) {
    start = piece.data() - PyBytes_AS_STRING(self->string);
  } else {
    start = piece.data() - PyUnicode_AsUTF8AndSize(self->string, NULL);
  }
#else
  start = piece.data() - PyString_AS_STRING(self->string);
#endif
  *o_start = start;
  *o_end = start + piece.length();
  return true;
}

/**
 * Return a pre-checked group number as a string, or default_obj
 * if it didn't participate in the match.
 */
static PyObject*
_group_get_i(MatchObject2* self, long idx, PyObject* default_obj)
{
  Py_ssize_t start;
  Py_ssize_t end;
  if (!_group_span(self, idx, &start, &end)) {
    Py_INCREF(default_obj);
    return default_obj;
  }
  return PySequence_GetSlice(self->string, start, end);
}

/**
 * Return n un-checked group number as a string.
 */
static PyObject*
_group_get_o(MatchObject2* self, PyObject* group)
{
  long idx;
  if (!_group_idx(self, group, &idx)) {
    return NULL;
  }
  return _group_get_i(self, idx, Py_None);
}


static PyObject*
match_group(MatchObject2* self, PyObject* args)
{
  long idx = 0;
  Py_ssize_t nargs = PyTuple_GET_SIZE(args);
  switch (nargs) {
    case 1:
      if (!_group_idx(self, PyTuple_GET_ITEM(args, 0), &idx)) {
        return NULL;
      }
      // Fall through.
    case 0:
      return _group_get_i(self, idx, Py_None);
    default:
      PyObject* ret = PyTuple_New(nargs);
      if (ret == NULL) {
        return NULL;
      }

      for (int i = 0; i < nargs; i++) {
        PyObject* group = _group_get_o(self, PyTuple_GET_ITEM(args, i));
        if (group == NULL) {
          Py_DECREF(ret);
          return NULL;
        }
        PyTuple_SET_ITEM(ret, i, group);
      }
      return ret;
  }
}

static PyObject*
match_groups(MatchObject2* self, PyObject* args, PyObject* kwds)
{
  static const char* kwlist[] = {
    "default",
    NULL};

  PyObject* default_obj = Py_None;

  if (!PyArg_ParseTupleAndKeywords(args, kwds, "|O", (char**)kwlist,
        &default_obj)) {
    return NULL;
  }

  int ngroups = ((RegexpObject2*)self->re)->re2_obj->NumberOfCapturingGroups();

  PyObject* ret = PyTuple_New(ngroups);
  if (ret == NULL) {
    return NULL;
  }

  for (int i = 1; i <= ngroups; i++) {
    PyObject* group = _group_get_i(self, i, default_obj);
    if (group == NULL) {
      Py_DECREF(ret);
      return NULL;
    }
    PyTuple_SET_ITEM(ret, i-1, group);
  }

  return ret;
}

static PyObject*
match_groupdict(MatchObject2* self, PyObject* args, PyObject* kwds)
{
  static const char* kwlist[] = {
    "default",
    NULL};

  PyObject* default_obj = Py_None;

  if (!PyArg_ParseTupleAndKeywords(args, kwds, "|O", (char**)kwlist,
        &default_obj)) {
    return NULL;
  }

  PyObject* ret = PyDict_New();
  if (ret == NULL) {
    return NULL;
  }

  const std::map<std::string, int>& name_map = ((RegexpObject2*)self->re)->re2_obj->NamedCapturingGroups();
  for (std::map<std::string, int>::const_iterator it = name_map.begin(); it != name_map.end(); ++it) {
    PyObject* group = _group_get_i(self, it->second, default_obj);
    if (group == NULL) {
      Py_DECREF(ret);
      return NULL;
    }
    // TODO: Group names with embedded zeroes?
    int res = PyDict_SetItemString(ret, it->first.data(), group);
    Py_DECREF(group);
    if (res < 0) {
      Py_DECREF(ret);
      return NULL;
    }
  }

  return ret;
}

enum span_mode_t { START, END, SPAN };

static PyObject*
_do_span(MatchObject2* self, PyObject* args, const char* name, span_mode_t mode)
{
  long idx = 0;
  PyObject* group = NULL;
  if (!PyArg_UnpackTuple(args, name, 0, 1,
        &group)) {
    return NULL;
  }
  if (group != NULL) {
    if (!_group_idx(self, group, &idx)) {
      return NULL;
    }
  }

  Py_ssize_t start = - 1;
  Py_ssize_t end = - 1;

  (void)_group_span(self, idx, &start, &end);
  switch (mode) {
    case START : return Py_BuildValue("n", start );
    case END   : return Py_BuildValue("n", end   );
    case SPAN:
      return Py_BuildValue("nn", start, end);
  }

  // Make gcc happy.
  return NULL;
}

static PyObject*
match_start(MatchObject2* self, PyObject* args)
{
  return _do_span(self, args, "start", START);
}

static PyObject*
match_end(MatchObject2* self, PyObject* args)
{
  return _do_span(self, args, "end", END);
}

static PyObject*
match_span(MatchObject2* self, PyObject* args)
{
  return _do_span(self, args, "span", SPAN);
}


static void
regexp_set_dealloc(RegexpSetObject2* self)
{
  delete self->re2_set_obj;
  PyObject_Del(self);
}

static PyObject*
regexp_set_new(PyTypeObject* type, PyObject* args, PyObject* kwds)
{
    int anchoring = RE2::UNANCHORED;

    if (!PyArg_ParseTuple(args, "|I", &anchoring)) {
      anchoring = -1;
    }

    switch (anchoring) {
      case RE2::UNANCHORED:
      case RE2::ANCHOR_START:
      case RE2::ANCHOR_BOTH:
        break;
      default:
        PyErr_SetString(PyExc_ValueError,
          "anchoring must be one of re2.UNANCHORED, re2.ANCHOR_START, or re2.ANCHOR_BOTH");
        return NULL;
    }

    RegexpSetObject2* self = (RegexpSetObject2*)type->tp_alloc(type, 0);

    if (self == NULL) {
      return NULL;
    }
    self->compiled = false;
    self->re2_set_obj = NULL;

    RE2::Options options;
    options.set_log_errors(false);

    self->re2_set_obj = new(nothrow) RE2::Set(options, (RE2::Anchor)anchoring);

    if (self->re2_set_obj == NULL) {
      PyErr_NoMemory();
      Py_DECREF(self);
      return NULL;
    }

    return (PyObject*)self;
}

static PyObject*
regexp_set_add(RegexpSetObject2* self, PyObject* pattern)
{
  if (self->compiled) {
    PyErr_SetString(PyExc_RuntimeError, "Can't add() on an already compiled Set");
    return NULL;
  }

  Py_ssize_t len_pattern;
#if PY_MAJOR_VERSION >= 3
  const char* raw_pattern = PyUnicode_AsUTF8AndSize(pattern, &len_pattern);
  if (!raw_pattern) {
    return NULL;
  }
#else
  const char* raw_pattern = PyString_AsString(pattern);
  if (!raw_pattern) {
    return NULL;
  }
  len_pattern = PyString_GET_SIZE(pattern);
#endif
  std::string add_error;
  int seq = self->re2_set_obj->Add(StringPiece(raw_pattern, (int)len_pattern), &add_error);

  if (seq < 0) {
    PyErr_SetString(PyExc_ValueError, add_error.c_str());
    return NULL;
  }

  return PyLong_FromLong(seq);
}

static PyObject*
regexp_set_compile(RegexpSetObject2* self)
{
  if (self->compiled) {
    Py_RETURN_NONE;
  }

  bool compiled = self->re2_set_obj->Compile();

  if (!compiled) {
    PyErr_SetString(PyExc_MemoryError, "Ran out of memory during regexp compile");
    return NULL;
  }

  self->compiled = true;
  Py_RETURN_NONE;
}

static PyObject*
regexp_set_match(RegexpSetObject2* self, PyObject* text)
{
  if (!self->compiled) {
    PyErr_SetString(PyExc_RuntimeError, "Can't match() on an uncompiled Set");
    return NULL;
  }

  const char* raw_text;
  Py_ssize_t len_text;
#if PY_MAJOR_VERSION >= 3
  if (PyUnicode_Check(text)) {
    raw_text = PyUnicode_AsUTF8AndSize(text, &len_text);
  } else if (PyBytes_Check(text)) {
    raw_text = PyBytes_AS_STRING(text);
    len_text = PyBytes_GET_SIZE(text);
  } else {
    PyErr_SetString(PyExc_TypeError, "expected str or bytes");
    return NULL;
  }
#else
  raw_text = PyString_AsString(text);
  if (raw_text == NULL) {
    return NULL;
  }
  len_text = PyString_GET_SIZE(text);
#endif

  std::vector<int> idxes;
  bool matched = self->re2_set_obj->Match(StringPiece(raw_text, (int)len_text), &idxes);

  if (matched) {
    PyObject* match_indexes = PyList_New(idxes.size());

    for(std::vector<int>::size_type i = 0; i < idxes.size(); ++i) {
      PyList_SET_ITEM(match_indexes, (Py_ssize_t)i, PyLong_FromLong(idxes[i]));
    }

    return match_indexes;
  } else {
    return PyList_New(0);
  }
}


static PyObject*
_compile(PyObject* self, PyObject* args)
{
  PyObject *pattern;
  PyObject *error_class;

  if (!PyArg_ParseTuple(args, "O!O:_compile",
                                   REGEX_OBJECT_TYPE, &pattern,
                                   &error_class)) {
    return NULL;
  }

  return create_regexp(self, pattern, error_class);
}

static PyObject*
escape(PyObject* self, PyObject* args)
{
  char *str;
  Py_ssize_t len;

  if (!PyArg_ParseTuple(args, "s#:escape", &str, &len)) {
    return NULL;
  }

  std::string esc(RE2::QuoteMeta(StringPiece(str, (int)len)));

#if PY_MAJOR_VERSION >= 3
  return PyUnicode_FromStringAndSize(esc.c_str(), esc.size());
#else
  return PyString_FromStringAndSize(esc.c_str(), esc.size());
#endif
}

static PyMethodDef methods[] = {
  {"_compile", (PyCFunction)_compile, METH_VARARGS | METH_KEYWORDS, NULL},
  {"escape", (PyCFunction)escape, METH_VARARGS,
   "Escape all potentially meaningful regexp characters."},
  {NULL}  /* Sentinel */
};


#if PY_MAJOR_VERSION >= 3
static struct PyModuleDef moduledef = {
  PyModuleDef_HEAD_INIT,
  "_re2",
  NULL,
  0,
  methods,
  NULL,
  NULL, // myextension_traverse,
  NULL, // myextension_clear,
  NULL
};

#define INITERROR return NULL
#else
#define INITERROR return
#endif


PyMODINIT_FUNC
#if PY_MAJOR_VERSION >= 3
PyInit__re2(void)
#else
init_re2(void)
#endif
{
  if (PyType_Ready(&Regexp_Type2) < 0) {
    INITERROR;
  }

  if (PyType_Ready(&Match_Type2) < 0) {
    INITERROR;
  }

  if (PyType_Ready(&RegexpSet_Type2) < 0) {
    INITERROR;
  }

#if PY_MAJOR_VERSION >= 3
  PyObject* mod = PyModule_Create(&moduledef);
#else
  PyObject* mod = Py_InitModule("_re2", methods);
#endif

  Py_INCREF(&RegexpSet_Type2);
  PyModule_AddObject(mod, "Set", (PyObject*)&RegexpSet_Type2);

  PyModule_AddIntConstant(mod, "UNANCHORED", RE2::UNANCHORED);
  PyModule_AddIntConstant(mod, "ANCHOR_START", RE2::ANCHOR_START);
  PyModule_AddIntConstant(mod, "ANCHOR_BOTH", RE2::ANCHOR_BOTH);
#if PY_MAJOR_VERSION >= 3
  return mod;
#endif
}
