/*
 parsers.c - efficient content parsing

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <ctype.h>
#include <stddef.h>
#include <string.h>

#include "util.h"

static char *versionerrortext = "Python minor version mismatch";

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

static char lowertable[128] = {
	'\x00', '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
	'\x08', '\x09', '\x0a', '\x0b', '\x0c', '\x0d', '\x0e', '\x0f',
	'\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
	'\x18', '\x19', '\x1a', '\x1b', '\x1c', '\x1d', '\x1e', '\x1f',
	'\x20', '\x21', '\x22', '\x23', '\x24', '\x25', '\x26', '\x27',
	'\x28', '\x29', '\x2a', '\x2b', '\x2c', '\x2d', '\x2e', '\x2f',
	'\x30', '\x31', '\x32', '\x33', '\x34', '\x35', '\x36', '\x37',
	'\x38', '\x39', '\x3a', '\x3b', '\x3c', '\x3d', '\x3e', '\x3f',
	'\x40',
	        '\x61', '\x62', '\x63', '\x64', '\x65', '\x66', '\x67', /* A-G */
	'\x68', '\x69', '\x6a', '\x6b', '\x6c', '\x6d', '\x6e', '\x6f', /* H-O */
	'\x70', '\x71', '\x72', '\x73', '\x74', '\x75', '\x76', '\x77', /* P-W */
	'\x78', '\x79', '\x7a',                                         /* X-Z */
	                        '\x5b', '\x5c', '\x5d', '\x5e', '\x5f',
	'\x60', '\x61', '\x62', '\x63', '\x64', '\x65', '\x66', '\x67',
	'\x68', '\x69', '\x6a', '\x6b', '\x6c', '\x6d', '\x6e', '\x6f',
	'\x70', '\x71', '\x72', '\x73', '\x74', '\x75', '\x76', '\x77',
	'\x78', '\x79', '\x7a', '\x7b', '\x7c', '\x7d', '\x7e', '\x7f'
};

static char uppertable[128] = {
	'\x00', '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
	'\x08', '\x09', '\x0a', '\x0b', '\x0c', '\x0d', '\x0e', '\x0f',
	'\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
	'\x18', '\x19', '\x1a', '\x1b', '\x1c', '\x1d', '\x1e', '\x1f',
	'\x20', '\x21', '\x22', '\x23', '\x24', '\x25', '\x26', '\x27',
	'\x28', '\x29', '\x2a', '\x2b', '\x2c', '\x2d', '\x2e', '\x2f',
	'\x30', '\x31', '\x32', '\x33', '\x34', '\x35', '\x36', '\x37',
	'\x38', '\x39', '\x3a', '\x3b', '\x3c', '\x3d', '\x3e', '\x3f',
	'\x40', '\x41', '\x42', '\x43', '\x44', '\x45', '\x46', '\x47',
	'\x48', '\x49', '\x4a', '\x4b', '\x4c', '\x4d', '\x4e', '\x4f',
	'\x50', '\x51', '\x52', '\x53', '\x54', '\x55', '\x56', '\x57',
	'\x58', '\x59', '\x5a', '\x5b', '\x5c', '\x5d', '\x5e', '\x5f',
	'\x60',
		'\x41', '\x42', '\x43', '\x44', '\x45', '\x46', '\x47', /* a-g */
	'\x48', '\x49', '\x4a', '\x4b', '\x4c', '\x4d', '\x4e', '\x4f', /* h-o */
	'\x50', '\x51', '\x52', '\x53', '\x54', '\x55', '\x56', '\x57', /* p-w */
	'\x58', '\x59', '\x5a', 					/* x-z */
				'\x7b', '\x7c', '\x7d', '\x7e', '\x7f'
};

static inline int hexdigit(const char *p, Py_ssize_t off)
{
	int8_t val = hextable[(unsigned char)p[off]];

	if (val >= 0) {
		return val;
	}

	PyErr_SetString(PyExc_ValueError, "input contains non-hex character");
	return 0;
}

/*
 * Turn a hex-encoded string into binary.
 */
PyObject *unhexlify(const char *str, int len)
{
	PyObject *ret;
	char *d;
	int i;

	ret = PyBytes_FromStringAndSize(NULL, len / 2);

	if (!ret)
		return NULL;

	d = PyBytes_AsString(ret);

	for (i = 0; i < len;) {
		int hi = hexdigit(str, i++);
		int lo = hexdigit(str, i++);
		*d++ = (hi << 4) | lo;
	}

	return ret;
}

static inline PyObject *_asciitransform(PyObject *str_obj,
					const char table[128],
					PyObject *fallback_fn)
{
	char *str, *newstr;
	Py_ssize_t i, len;
	PyObject *newobj = NULL;
	PyObject *ret = NULL;

	str = PyBytes_AS_STRING(str_obj);
	len = PyBytes_GET_SIZE(str_obj);

	newobj = PyBytes_FromStringAndSize(NULL, len);
	if (!newobj)
		goto quit;

	newstr = PyBytes_AS_STRING(newobj);

	for (i = 0; i < len; i++) {
		char c = str[i];
		if (c & 0x80) {
			if (fallback_fn != NULL) {
				ret = PyObject_CallFunctionObjArgs(fallback_fn,
					str_obj, NULL);
			} else {
				PyObject *err = PyUnicodeDecodeError_Create(
					"ascii", str, len, i, (i + 1),
					"unexpected code byte");
				PyErr_SetObject(PyExc_UnicodeDecodeError, err);
				Py_XDECREF(err);
			}
			goto quit;
		}
		newstr[i] = table[(unsigned char)c];
	}

	ret = newobj;
	Py_INCREF(ret);
quit:
	Py_XDECREF(newobj);
	return ret;
}

static PyObject *asciilower(PyObject *self, PyObject *args)
{
	PyObject *str_obj;
	if (!PyArg_ParseTuple(args, "O!:asciilower", &PyBytes_Type, &str_obj))
		return NULL;
	return _asciitransform(str_obj, lowertable, NULL);
}

static PyObject *asciiupper(PyObject *self, PyObject *args)
{
	PyObject *str_obj;
	if (!PyArg_ParseTuple(args, "O!:asciiupper", &PyBytes_Type, &str_obj))
		return NULL;
	return _asciitransform(str_obj, uppertable, NULL);
}

static inline PyObject *_dict_new_presized(Py_ssize_t expected_size)
{
	/* _PyDict_NewPresized expects a minused parameter, but it actually
	   creates a dictionary that's the nearest power of two bigger than the
	   parameter. For example, with the initial minused = 1000, the
	   dictionary created has size 1024. Of course in a lot of cases that
	   can be greater than the maximum load factor Python's dict object
	   expects (= 2/3), so as soon as we cross the threshold we'll resize
	   anyway. So create a dictionary that's at least 3/2 the size. */
	return _PyDict_NewPresized(((1 + expected_size) / 2) * 3);
}

static PyObject *dict_new_presized(PyObject *self, PyObject *args)
{
	Py_ssize_t expected_size;

	if (!PyArg_ParseTuple(args, "n:make_presized_dict", &expected_size))
		return NULL;

	return _dict_new_presized(expected_size);
}

static PyObject *make_file_foldmap(PyObject *self, PyObject *args)
{
	PyObject *dmap, *spec_obj, *normcase_fallback;
	PyObject *file_foldmap = NULL;
	enum normcase_spec spec;
	PyObject *k, *v;
	dirstateTupleObject *tuple;
	Py_ssize_t pos = 0;
	const char *table;

	if (!PyArg_ParseTuple(args, "O!O!O!:make_file_foldmap",
			      &PyDict_Type, &dmap,
			      &PyInt_Type, &spec_obj,
			      &PyFunction_Type, &normcase_fallback))
		goto quit;

	spec = (int)PyInt_AS_LONG(spec_obj);
	switch (spec) {
	case NORMCASE_LOWER:
		table = lowertable;
		break;
	case NORMCASE_UPPER:
		table = uppertable;
		break;
	case NORMCASE_OTHER:
		table = NULL;
		break;
	default:
		PyErr_SetString(PyExc_TypeError, "invalid normcasespec");
		goto quit;
	}

	/* Add some more entries to deal with additions outside this
	   function. */
	file_foldmap = _dict_new_presized((PyDict_Size(dmap) / 10) * 11);
	if (file_foldmap == NULL)
		goto quit;

	while (PyDict_Next(dmap, &pos, &k, &v)) {
		if (!dirstate_tuple_check(v)) {
			PyErr_SetString(PyExc_TypeError,
					"expected a dirstate tuple");
			goto quit;
		}

		tuple = (dirstateTupleObject *)v;
		if (tuple->state != 'r') {
			PyObject *normed;
			if (table != NULL) {
				normed = _asciitransform(k, table,
					normcase_fallback);
			} else {
				normed = PyObject_CallFunctionObjArgs(
					normcase_fallback, k, NULL);
			}

			if (normed == NULL)
				goto quit;
			if (PyDict_SetItem(file_foldmap, normed, k) == -1)
				goto quit;
		}
	}
	return file_foldmap;
quit:
	Py_XDECREF(file_foldmap);
	return NULL;
}

/*
 * This code assumes that a manifest is stitched together with newline
 * ('\n') characters.
 */
static PyObject *parse_manifest(PyObject *self, PyObject *args)
{
	PyObject *mfdict, *fdict;
	char *str, *start, *end;
	int len;

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_manifest",
			      &PyDict_Type, &mfdict,
			      &PyDict_Type, &fdict,
			      &str, &len))
		goto quit;

	start = str;
	end = str + len;
	while (start < end) {
		PyObject *file = NULL, *node = NULL;
		PyObject *flags = NULL;
		char *zero = NULL, *newline = NULL;
		ptrdiff_t nlen;

		zero = memchr(start, '\0', end - start);
		if (!zero) {
			PyErr_SetString(PyExc_ValueError,
					"manifest entry has no separator");
			goto quit;
		}

		newline = memchr(zero + 1, '\n', end - (zero + 1));
		if (!newline) {
			PyErr_SetString(PyExc_ValueError,
					"manifest contains trailing garbage");
			goto quit;
		}

		file = PyBytes_FromStringAndSize(start, zero - start);

		if (!file)
			goto bail;

		nlen = newline - zero - 1;

		node = unhexlify(zero + 1, nlen > 40 ? 40 : (int)nlen);
		if (!node)
			goto bail;

		if (nlen > 40) {
			flags = PyBytes_FromStringAndSize(zero + 41,
							   nlen - 40);
			if (!flags)
				goto bail;

			if (PyDict_SetItem(fdict, file, flags) == -1)
				goto bail;
		}

		if (PyDict_SetItem(mfdict, file, node) == -1)
			goto bail;

		start = newline + 1;

		Py_XDECREF(flags);
		Py_XDECREF(node);
		Py_XDECREF(file);
		continue;
	bail:
		Py_XDECREF(flags);
		Py_XDECREF(node);
		Py_XDECREF(file);
		goto quit;
	}

	Py_INCREF(Py_None);
	return Py_None;
quit:
	return NULL;
}

static inline dirstateTupleObject *make_dirstate_tuple(char state, int mode,
						       int size, int mtime)
{
	dirstateTupleObject *t = PyObject_New(dirstateTupleObject,
					      &dirstateTupleType);
	if (!t)
		return NULL;
	t->state = state;
	t->mode = mode;
	t->size = size;
	t->mtime = mtime;
	return t;
}

static PyObject *dirstate_tuple_new(PyTypeObject *subtype, PyObject *args,
				    PyObject *kwds)
{
	/* We do all the initialization here and not a tp_init function because
	 * dirstate_tuple is immutable. */
	dirstateTupleObject *t;
	char state;
	int size, mode, mtime;
	if (!PyArg_ParseTuple(args, "ciii", &state, &mode, &size, &mtime))
		return NULL;

	t = (dirstateTupleObject *)subtype->tp_alloc(subtype, 1);
	if (!t)
		return NULL;
	t->state = state;
	t->mode = mode;
	t->size = size;
	t->mtime = mtime;

	return (PyObject *)t;
}

static void dirstate_tuple_dealloc(PyObject *o)
{
	PyObject_Del(o);
}

static Py_ssize_t dirstate_tuple_length(PyObject *o)
{
	return 4;
}

static PyObject *dirstate_tuple_item(PyObject *o, Py_ssize_t i)
{
	dirstateTupleObject *t = (dirstateTupleObject *)o;
	switch (i) {
	case 0:
		return PyBytes_FromStringAndSize(&t->state, 1);
	case 1:
		return PyInt_FromLong(t->mode);
	case 2:
		return PyInt_FromLong(t->size);
	case 3:
		return PyInt_FromLong(t->mtime);
	default:
		PyErr_SetString(PyExc_IndexError, "index out of range");
		return NULL;
	}
}

static PySequenceMethods dirstate_tuple_sq = {
	dirstate_tuple_length,     /* sq_length */
	0,                         /* sq_concat */
	0,                         /* sq_repeat */
	dirstate_tuple_item,       /* sq_item */
	0,                         /* sq_ass_item */
	0,                         /* sq_contains */
	0,                         /* sq_inplace_concat */
	0                          /* sq_inplace_repeat */
};

PyTypeObject dirstateTupleType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"dirstate_tuple",          /* tp_name */
	sizeof(dirstateTupleObject),/* tp_basicsize */
	0,                         /* tp_itemsize */
	(destructor)dirstate_tuple_dealloc, /* tp_dealloc */
	0,                         /* tp_print */
	0,                         /* tp_getattr */
	0,                         /* tp_setattr */
	0,                         /* tp_compare */
	0,                         /* tp_repr */
	0,                         /* tp_as_number */
	&dirstate_tuple_sq,        /* tp_as_sequence */
	0,                         /* tp_as_mapping */
	0,                         /* tp_hash  */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT,        /* tp_flags */
	"dirstate tuple",          /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	0,                         /* tp_methods */
	0,                         /* tp_members */
	0,                         /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	0,                         /* tp_init */
	0,                         /* tp_alloc */
	dirstate_tuple_new,        /* tp_new */
};

static PyObject *parse_dirstate(PyObject *self, PyObject *args)
{
	PyObject *dmap, *cmap, *parents = NULL, *ret = NULL;
	PyObject *fname = NULL, *cname = NULL, *entry = NULL;
	char state, *cur, *str, *cpos;
	int mode, size, mtime;
	unsigned int flen, len, pos = 40;
	int readlen;

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_dirstate",
			      &PyDict_Type, &dmap,
			      &PyDict_Type, &cmap,
			      &str, &readlen))
		goto quit;

	if (readlen < 0)
		goto quit;

	len = readlen;

	/* read parents */
	if (len < 40)
		goto quit;

	parents = Py_BuildValue("s#s#", str, 20, str + 20, 20);
	if (!parents)
		goto quit;

	/* read filenames */
	while (pos >= 40 && pos < len) {
		cur = str + pos;
		/* unpack header */
		state = *cur;
		mode = getbe32(cur + 1);
		size = getbe32(cur + 5);
		mtime = getbe32(cur + 9);
		flen = getbe32(cur + 13);
		pos += 17;
		cur += 17;
		if (flen > len - pos) {
			PyErr_SetString(PyExc_ValueError, "overflow in dirstate");
			goto quit;
		}

		entry = (PyObject *)make_dirstate_tuple(state, mode, size,
							mtime);
		cpos = memchr(cur, 0, flen);
		if (cpos) {
			fname = PyBytes_FromStringAndSize(cur, cpos - cur);
			cname = PyBytes_FromStringAndSize(cpos + 1,
							   flen - (cpos - cur) - 1);
			if (!fname || !cname ||
			    PyDict_SetItem(cmap, fname, cname) == -1 ||
			    PyDict_SetItem(dmap, fname, entry) == -1)
				goto quit;
			Py_DECREF(cname);
		} else {
			fname = PyBytes_FromStringAndSize(cur, flen);
			if (!fname ||
			    PyDict_SetItem(dmap, fname, entry) == -1)
				goto quit;
		}
		Py_DECREF(fname);
		Py_DECREF(entry);
		fname = cname = entry = NULL;
		pos += flen;
	}

	ret = parents;
	Py_INCREF(ret);
quit:
	Py_XDECREF(fname);
	Py_XDECREF(cname);
	Py_XDECREF(entry);
	Py_XDECREF(parents);
	return ret;
}

/*
 * Efficiently pack a dirstate object into its on-disk format.
 */
static PyObject *pack_dirstate(PyObject *self, PyObject *args)
{
	PyObject *packobj = NULL;
	PyObject *map, *copymap, *pl, *mtime_unset = NULL;
	Py_ssize_t nbytes, pos, l;
	PyObject *k, *v = NULL, *pn;
	char *p, *s;
	double now;

	if (!PyArg_ParseTuple(args, "O!O!Od:pack_dirstate",
			      &PyDict_Type, &map, &PyDict_Type, &copymap,
			      &pl, &now))
		return NULL;

	if (!PySequence_Check(pl) || PySequence_Size(pl) != 2) {
		PyErr_SetString(PyExc_TypeError, "expected 2-element sequence");
		return NULL;
	}

	/* Figure out how much we need to allocate. */
	for (nbytes = 40, pos = 0; PyDict_Next(map, &pos, &k, &v);) {
		PyObject *c;
		if (!PyString_Check(k)) {
			PyErr_SetString(PyExc_TypeError, "expected string key");
			goto bail;
		}
		nbytes += PyString_GET_SIZE(k) + 17;
		c = PyDict_GetItem(copymap, k);
		if (c) {
			if (!PyString_Check(c)) {
				PyErr_SetString(PyExc_TypeError,
						"expected string key");
				goto bail;
			}
			nbytes += PyString_GET_SIZE(c) + 1;
		}
	}

	packobj = PyString_FromStringAndSize(NULL, nbytes);
	if (packobj == NULL)
		goto bail;

	p = PyString_AS_STRING(packobj);

	pn = PySequence_ITEM(pl, 0);
	if (PyString_AsStringAndSize(pn, &s, &l) == -1 || l != 20) {
		PyErr_SetString(PyExc_TypeError, "expected a 20-byte hash");
		goto bail;
	}
	memcpy(p, s, l);
	p += 20;
	pn = PySequence_ITEM(pl, 1);
	if (PyString_AsStringAndSize(pn, &s, &l) == -1 || l != 20) {
		PyErr_SetString(PyExc_TypeError, "expected a 20-byte hash");
		goto bail;
	}
	memcpy(p, s, l);
	p += 20;

	for (pos = 0; PyDict_Next(map, &pos, &k, &v); ) {
		dirstateTupleObject *tuple;
		char state;
		uint32_t mode, size, mtime;
		Py_ssize_t len, l;
		PyObject *o;
		char *t;

		if (!dirstate_tuple_check(v)) {
			PyErr_SetString(PyExc_TypeError,
					"expected a dirstate tuple");
			goto bail;
		}
		tuple = (dirstateTupleObject *)v;

		state = tuple->state;
		mode = tuple->mode;
		size = tuple->size;
		mtime = tuple->mtime;
		if (state == 'n' && mtime == (uint32_t)now) {
			/* See pure/parsers.py:pack_dirstate for why we do
			 * this. */
			mtime = -1;
			mtime_unset = (PyObject *)make_dirstate_tuple(
				state, mode, size, mtime);
			if (!mtime_unset)
				goto bail;
			if (PyDict_SetItem(map, k, mtime_unset) == -1)
				goto bail;
			Py_DECREF(mtime_unset);
			mtime_unset = NULL;
		}
		*p++ = state;
		putbe32(mode, p);
		putbe32(size, p + 4);
		putbe32(mtime, p + 8);
		t = p + 12;
		p += 16;
		len = PyString_GET_SIZE(k);
		memcpy(p, PyString_AS_STRING(k), len);
		p += len;
		o = PyDict_GetItem(copymap, k);
		if (o) {
			*p++ = '\0';
			l = PyString_GET_SIZE(o);
			memcpy(p, PyString_AS_STRING(o), l);
			p += l;
			len += l + 1;
		}
		putbe32((uint32_t)len, t);
	}

	pos = p - PyString_AS_STRING(packobj);
	if (pos != nbytes) {
		PyErr_Format(PyExc_SystemError, "bad dirstate size: %ld != %ld",
                             (long)pos, (long)nbytes);
		goto bail;
	}

	return packobj;
bail:
	Py_XDECREF(mtime_unset);
	Py_XDECREF(packobj);
	Py_XDECREF(v);
	return NULL;
}

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
 * This class has two behaviours.
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
	PyObject *data;        /* raw bytes of index */
	PyObject **cache;      /* cached tuples */
	const char **offsets;  /* populated on demand */
	Py_ssize_t raw_length; /* original number of elements */
	Py_ssize_t length;     /* current number of elements */
	PyObject *added;       /* populated on demand */
	PyObject *headrevs;    /* cache, invalidated on changes */
	PyObject *filteredrevs;/* filtered revs set */
	nodetree *nt;          /* base-16 trie */
	int ntlength;          /* # nodes in use */
	int ntcapacity;        /* # nodes allocated */
	int ntdepth;           /* maximum depth of tree */
	int ntsplits;          /* # splits performed */
	int ntrev;             /* last rev scanned */
	int ntlookups;         /* # lookups */
	int ntmisses;          /* # lookups that miss the cache */
	int inlined;
} indexObject;

static Py_ssize_t index_length(const indexObject *self)
{
	if (self->added == NULL)
		return self->length;
	return self->length + PyList_GET_SIZE(self->added);
}

static PyObject *nullentry;
static const char nullid[20];

static Py_ssize_t inline_scan(indexObject *self, const char **offsets);

#if LONG_MAX == 0x7fffffffL
static char *tuple_format = "Kiiiiiis#";
#else
static char *tuple_format = "kiiiiiis#";
#endif

/* A RevlogNG v1 index entry is 64 bytes long. */
static const long v1_hdrsize = 64;

/*
 * Return a pointer to the beginning of a RevlogNG record.
 */
static const char *index_deref(indexObject *self, Py_ssize_t pos)
{
	if (self->inlined && pos > 0) {
		if (self->offsets == NULL) {
			self->offsets = malloc(self->raw_length *
					       sizeof(*self->offsets));
			if (self->offsets == NULL)
				return (const char *)PyErr_NoMemory();
			inline_scan(self, self->offsets);
		}
		return self->offsets[pos];
	}

	return PyString_AS_STRING(self->data) + pos * v1_hdrsize;
}

static inline int index_get_parents(indexObject *self, Py_ssize_t rev,
				    int *ps, int maxrev)
{
	if (rev >= self->length - 1) {
		PyObject *tuple = PyList_GET_ITEM(self->added,
						  rev - self->length + 1);
		ps[0] = (int)PyInt_AS_LONG(PyTuple_GET_ITEM(tuple, 5));
		ps[1] = (int)PyInt_AS_LONG(PyTuple_GET_ITEM(tuple, 6));
	} else {
		const char *data = index_deref(self, rev);
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
static PyObject *index_get(indexObject *self, Py_ssize_t pos)
{
	uint64_t offset_flags;
	int comp_len, uncomp_len, base_rev, link_rev, parent_1, parent_2;
	const char *c_node_id;
	const char *data;
	Py_ssize_t length = index_length(self);
	PyObject *entry;

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
		PyObject *obj;
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
		self->cache = calloc(self->raw_length, sizeof(PyObject *));
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

	entry = Py_BuildValue(tuple_format, offset_flags, comp_len,
			      uncomp_len, base_rev, link_rev,
			      parent_1, parent_2, c_node_id, 20);

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
static const char *index_node(indexObject *self, Py_ssize_t pos)
{
	Py_ssize_t length = index_length(self);
	const char *data;

	if (pos == length - 1 || pos == INT_MAX)
		return nullid;

	if (pos >= length)
		return NULL;

	if (pos >= self->length - 1) {
		PyObject *tuple, *str;
		tuple = PyList_GET_ITEM(self->added, pos - self->length + 1);
		str = PyTuple_GetItem(tuple, 7);
		return str ? PyString_AS_STRING(str) : NULL;
	}

	data = index_deref(self, pos);
	return data ? data + 32 : NULL;
}

static int nt_insert(indexObject *self, const char *node, int rev);

static int node_check(PyObject *obj, char **node, Py_ssize_t *nodelen)
{
	if (PyString_AsStringAndSize(obj, node, nodelen) == -1)
		return -1;
	if (*nodelen == 20)
		return 0;
	PyErr_SetString(PyExc_ValueError, "20-byte hash required");
	return -1;
}

static PyObject *index_insert(indexObject *self, PyObject *args)
{
	PyObject *obj;
	char *node;
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
		PyErr_SetString(PyExc_IndexError,
				"insert only supported at index -1");
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

static void _index_clearcaches(indexObject *self)
{
	if (self->cache) {
		Py_ssize_t i;

		for (i = 0; i < self->raw_length; i++)
			Py_CLEAR(self->cache[i]);
		free(self->cache);
		self->cache = NULL;
	}
	if (self->offsets) {
		free(self->offsets);
		self->offsets = NULL;
	}
	if (self->nt) {
		free(self->nt);
		self->nt = NULL;
	}
	Py_CLEAR(self->headrevs);
}

static PyObject *index_clearcaches(indexObject *self)
{
	_index_clearcaches(self);
	self->ntlength = self->ntcapacity = 0;
	self->ntdepth = self->ntsplits = 0;
	self->ntrev = -1;
	self->ntlookups = self->ntmisses = 0;
	Py_RETURN_NONE;
}

static PyObject *index_stats(indexObject *self)
{
	PyObject *obj = PyDict_New();
	PyObject *t = NULL;

	if (obj == NULL)
		return NULL;

#define istat(__n, __d) \
	t = PyInt_FromSsize_t(self->__n); \
	if (!t) \
		goto bail; \
	if (PyDict_SetItemString(obj, __d, t) == -1) \
		goto bail; \
	Py_DECREF(t);

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
static PyObject *list_copy(PyObject *list)
{
	Py_ssize_t len = PyList_GET_SIZE(list);
	PyObject *newlist = PyList_New(len);
	Py_ssize_t i;

	if (newlist == NULL)
		return NULL;

	for (i = 0; i < len; i++) {
		PyObject *obj = PyList_GET_ITEM(list, i);
		Py_INCREF(obj);
		PyList_SET_ITEM(newlist, i, obj);
	}

	return newlist;
}

/* arg should be Py_ssize_t but Python 2.4 do not support the n format */
static int check_filter(PyObject *filter, unsigned long arg) {
	if (filter) {
		PyObject *arglist, *result;
		int isfiltered;

		arglist = Py_BuildValue("(k)", arg);
		if (!arglist) {
			return -1;
		}

		result = PyEval_CallObject(filter, arglist);
		Py_DECREF(arglist);
		if (!result) {
			return -1;
		}

		/* PyObject_IsTrue returns 1 if true, 0 if false, -1 if error,
		 * same as this function, so we can just return it directly.*/
		isfiltered = PyObject_IsTrue(result);
		Py_DECREF(result);
		return isfiltered;
	} else {
		return 0;
	}
}

static Py_ssize_t add_roots_get_min(indexObject *self, PyObject *list,
                                    Py_ssize_t marker, char *phases)
{
	PyObject *iter = NULL;
	PyObject *iter_item = NULL;
	Py_ssize_t min_idx = index_length(self) + 1;
	long iter_item_long;

	if (PyList_GET_SIZE(list) != 0) {
		iter = PyObject_GetIter(list);
		if (iter == NULL)
			return -2;
		while ((iter_item = PyIter_Next(iter)))
		{
			iter_item_long = PyInt_AS_LONG(iter_item);
			Py_DECREF(iter_item);
			if (iter_item_long < min_idx)
				min_idx = iter_item_long;
			phases[iter_item_long] = marker;
		}
		Py_DECREF(iter);
	}

	return min_idx;
}

static inline void set_phase_from_parents(char *phases, int parent_1,
                                          int parent_2, Py_ssize_t i)
{
	if (parent_1 >= 0 && phases[parent_1] > phases[i])
		phases[i] = phases[parent_1];
	if (parent_2 >= 0 && phases[parent_2] > phases[i])
		phases[i] = phases[parent_2];
}

static PyObject *compute_phases_map_sets(indexObject *self, PyObject *args)
{
	PyObject *roots = Py_None;
	PyObject *ret = NULL;
	PyObject *phaseslist = NULL;
	PyObject *phaseroots = NULL;
	PyObject *phaseset = NULL;
	PyObject *phasessetlist = NULL;
	Py_ssize_t len = index_length(self) - 1;
	Py_ssize_t numphase = 0;
	Py_ssize_t minrevallphases = 0;
	Py_ssize_t minrevphase = 0;
	Py_ssize_t i = 0;
	char *phases = NULL;
	long phase;

	if (!PyArg_ParseTuple(args, "O", &roots))
		goto release_none;
	if (roots == NULL || !PyList_Check(roots))
		goto release_none;

	phases = calloc(len, 1); /* phase per rev: {0: public, 1: draft, 2: secret} */
	if (phases == NULL)
		goto release_none;
	/* Put the phase information of all the roots in phases */
	numphase = PyList_GET_SIZE(roots)+1;
	minrevallphases = len + 1;
	phasessetlist = PyList_New(numphase);
	if (phasessetlist == NULL)
		goto release_none;

	PyList_SET_ITEM(phasessetlist, 0, Py_None);
	Py_INCREF(Py_None);

	for (i = 0; i < numphase-1; i++) {
		phaseroots = PyList_GET_ITEM(roots, i);
		phaseset = PySet_New(NULL);
		if (phaseset == NULL)
			goto release_phasesetlist;
		PyList_SET_ITEM(phasessetlist, i+1, phaseset);
		if (!PyList_Check(phaseroots))
			goto release_phasesetlist;
		minrevphase = add_roots_get_min(self, phaseroots, i+1, phases);
		if (minrevphase == -2) /* Error from add_roots_get_min */
			goto release_phasesetlist;
		minrevallphases = MIN(minrevallphases, minrevphase);
	}
	/* Propagate the phase information from the roots to the revs */
	if (minrevallphases != -1) {
		int parents[2];
		for (i = minrevallphases; i < len; i++) {
			if (index_get_parents(self, i, parents, len - 1) < 0)
				goto release_phasesetlist;
			set_phase_from_parents(phases, parents[0], parents[1], i);
		}
	}
	/* Transform phase list to a python list */
	phaseslist = PyList_New(len);
	if (phaseslist == NULL)
		goto release_phasesetlist;
	for (i = 0; i < len; i++) {
		phase = phases[i];
		/* We only store the sets of phase for non public phase, the public phase
		 * is computed as a difference */
		if (phase != 0) {
			phaseset = PyList_GET_ITEM(phasessetlist, phase);
			PySet_Add(phaseset, PyInt_FromLong(i));
		}
		PyList_SET_ITEM(phaseslist, i, PyInt_FromLong(phase));
	}
	ret = PyList_New(2);
	if (ret == NULL)
		goto release_phaseslist;

	PyList_SET_ITEM(ret, 0, phaseslist);
	PyList_SET_ITEM(ret, 1, phasessetlist);
	/* We don't release phaseslist and phasessetlist as we return them to
	 * python */
	goto release_phases;

release_phaseslist:
	Py_XDECREF(phaseslist);
release_phasesetlist:
	Py_XDECREF(phasessetlist);
release_phases:
	free(phases);
release_none:
	return ret;
}

static PyObject *index_headrevs(indexObject *self, PyObject *args)
{
	Py_ssize_t i, j, len;
	char *nothead = NULL;
	PyObject *heads = NULL;
	PyObject *filter = NULL;
	PyObject *filteredrevs = Py_None;

	if (!PyArg_ParseTuple(args, "|O", &filteredrevs)) {
		return NULL;
	}

	if (self->headrevs && filteredrevs == self->filteredrevs)
		return list_copy(self->headrevs);

	Py_DECREF(self->filteredrevs);
	self->filteredrevs = filteredrevs;
	Py_INCREF(filteredrevs);

	if (filteredrevs != Py_None) {
		filter = PyObject_GetAttrString(filteredrevs, "__contains__");
		if (!filter) {
			PyErr_SetString(PyExc_TypeError,
				"filteredrevs has no attribute __contains__");
			goto bail;
		}
	}

	len = index_length(self) - 1;
	heads = PyList_New(0);
	if (heads == NULL)
		goto bail;
	if (len == 0) {
		PyObject *nullid = PyInt_FromLong(-1);
		if (nullid == NULL || PyList_Append(heads, nullid) == -1) {
			Py_XDECREF(nullid);
			goto bail;
		}
		goto done;
	}

	nothead = calloc(len, 1);
	if (nothead == NULL)
		goto bail;

	for (i = 0; i < len; i++) {
		int isfiltered;
		int parents[2];

		isfiltered = check_filter(filter, i);
		if (isfiltered == -1) {
			PyErr_SetString(PyExc_TypeError,
				"unable to check filter");
			goto bail;
		}

		if (isfiltered) {
			nothead[i] = 1;
			continue;
		}

		if (index_get_parents(self, i, parents, len - 1) < 0)
			goto bail;
		for (j = 0; j < 2; j++) {
			if (parents[j] >= 0)
				nothead[parents[j]] = 1;
		}
	}

	for (i = 0; i < len; i++) {
		PyObject *head;

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
	Py_XDECREF(filter);
	free(nothead);
	return list_copy(self->headrevs);
bail:
	Py_XDECREF(filter);
	Py_XDECREF(heads);
	free(nothead);
	return NULL;
}

static inline int nt_level(const char *node, Py_ssize_t level)
{
	int v = node[level>>1];
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
static int nt_find(indexObject *self, const char *node, Py_ssize_t nodelen,
		   int hex)
{
	int (*getnybble)(const char *, Py_ssize_t) = hex ? hexdigit : nt_level;
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
		nodetree *n = &self->nt[off];
		int v = n->children[k];

		if (v < 0) {
			const char *n;
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

static int nt_new(indexObject *self)
{
	if (self->ntlength == self->ntcapacity) {
		if (self->ntcapacity >= INT_MAX / (sizeof(nodetree) * 2)) {
			PyErr_SetString(PyExc_MemoryError,
					"overflow in nt_new");
			return -1;
		}
		self->ntcapacity *= 2;
		self->nt = realloc(self->nt,
				   self->ntcapacity * sizeof(nodetree));
		if (self->nt == NULL) {
			PyErr_SetString(PyExc_MemoryError, "out of memory");
			return -1;
		}
		memset(&self->nt[self->ntlength], 0,
		       sizeof(nodetree) * (self->ntcapacity - self->ntlength));
	}
	return self->ntlength++;
}

static int nt_insert(indexObject *self, const char *node, int rev)
{
	int level = 0;
	int off = 0;

	while (level < 40) {
		int k = nt_level(node, level);
		nodetree *n;
		int v;

		n = &self->nt[off];
		v = n->children[k];

		if (v == 0) {
			n->children[k] = -rev - 1;
			return 0;
		}
		if (v < 0) {
			const char *oldnode = index_node(self, -(v + 1));
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

static int nt_init(indexObject *self)
{
	if (self->nt == NULL) {
		if (self->raw_length > INT_MAX / sizeof(nodetree)) {
			PyErr_SetString(PyExc_ValueError, "overflow in nt_init");
			return -1;
		}
		self->ntcapacity = self->raw_length < 4
			? 4 : (int)self->raw_length / 2;

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
static int index_find_node(indexObject *self,
			   const char *node, Py_ssize_t nodelen)
{
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
			const char *n = index_node(self, rev);
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
			const char *n = index_node(self, rev);
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

static void raise_revlog_error(void)
{
	PyObject *mod = NULL, *dict = NULL, *errclass = NULL;

	mod = PyImport_ImportModule("mercurial.error");
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
		PyErr_SetString(PyExc_SystemError,
				"could not find RevlogError");
		goto cleanup;
	}

	/* value of exception is ignored by callers */
	PyErr_SetString(errclass, "RevlogError");

cleanup:
	Py_XDECREF(dict);
	Py_XDECREF(mod);
}

static PyObject *index_getitem(indexObject *self, PyObject *value)
{
	char *node;
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

static int nt_partialmatch(indexObject *self, const char *node,
			   Py_ssize_t nodelen)
{
	int rev;

	if (nt_init(self) == -1)
		return -3;

	if (self->ntrev > 0) {
		/* ensure that the radix tree is fully populated */
		for (rev = self->ntrev - 1; rev >= 0; rev--) {
			const char *n = index_node(self, rev);
			if (n == NULL)
				return -2;
			if (nt_insert(self, n, rev) == -1)
				return -3;
		}
		self->ntrev = rev;
	}

	return nt_find(self, node, nodelen, 1);
}

static PyObject *index_partialmatch(indexObject *self, PyObject *args)
{
	const char *fullnode;
	int nodelen;
	char *node;
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
	case -3:
		return NULL;
	case -2:
		Py_RETURN_NONE;
	case -1:
		return PyString_FromStringAndSize(nullid, 20);
	}

	fullnode = index_node(self, rev);
	if (fullnode == NULL) {
		PyErr_Format(PyExc_IndexError,
			     "could not access rev %d", rev);
		return NULL;
	}
	return PyString_FromStringAndSize(fullnode, 20);
}

static PyObject *index_m_get(indexObject *self, PyObject *args)
{
	Py_ssize_t nodelen;
	PyObject *val;
	char *node;
	int rev;

	if (!PyArg_ParseTuple(args, "O", &val))
		return NULL;
	if (node_check(val, &node, &nodelen) == -1)
		return NULL;
	rev = index_find_node(self, node, nodelen);
	if (rev ==  -3)
		return NULL;
	if (rev == -2)
		Py_RETURN_NONE;
	return PyInt_FromLong(rev);
}

static int index_contains(indexObject *self, PyObject *value)
{
	char *node;
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
static PyObject *find_gca_candidates(indexObject *self, const int *revs,
				     int revcount)
{
	const bitmask allseen = (1ull << revcount) - 1;
	const bitmask poison = 1ull << revcount;
	PyObject *gca = PyList_New(0);
	int i, v, interesting;
	int maxrev = -1;
	bitmask sp;
	bitmask *seen;

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
				PyObject *obj = PyInt_FromLong(v);
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
				}
				else if (sp != sv)
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
static PyObject *find_deepest(indexObject *self, PyObject *revs)
{
	const Py_ssize_t revcount = PyList_GET_SIZE(revs);
	static const Py_ssize_t capacity = 24;
	int *depth, *interesting = NULL;
	int i, j, v, ninteresting;
	PyObject *dict = NULL, *keys = NULL;
	long *seen = NULL;
	int maxrev = -1;
	long final;

	if (revcount > capacity) {
		PyErr_Format(PyExc_OverflowError,
			     "bitset size (%ld) > capacity (%ld)",
			     (long)revcount, (long)capacity);
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

	interesting = calloc(sizeof(*interesting), 2 << revcount);
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
			long nsp, sp;
			int dp;

			if (p == -1)
				continue;

			dp = depth[p];
			nsp = sp = seen[p];
			if (dp <= dv) {
				depth[p] = dv + 1;
				if (sp != sv) {
					interesting[sv] += 1;
					nsp = seen[p] = sv;
					if (sp) {
						interesting[sp] -= 1;
						if (interesting[sp] == 0)
							ninteresting -= 1;
					}
				}
			}
			else if (dv == dp - 1) {
				nsp = sp | sv;
				if (nsp == sp)
					continue;
				seen[p] = nsp;
				interesting[sp] -= 1;
				if (interesting[sp] == 0 && interesting[nsp] > 0)
					ninteresting -= 1;
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
		PyObject *key;

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
static PyObject *index_commonancestorsheads(indexObject *self, PyObject *args)
{
	PyObject *ret = NULL;
	Py_ssize_t argcount, i, len;
	bitmask repeat = 0;
	int revcount = 0;
	int *revs;

	argcount = PySequence_Length(args);
	revs = malloc(argcount * sizeof(*revs));
	if (argcount > 0 && revs == NULL)
		return PyErr_NoMemory();
	len = index_length(self) - 1;

	for (i = 0; i < argcount; i++) {
		static const int capacity = 24;
		PyObject *obj = PySequence_GetItem(args, i);
		bitmask x;
		long val;

		if (!PyInt_Check(obj)) {
			PyErr_SetString(PyExc_TypeError,
					"arguments must all be ints");
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
			PyErr_SetString(PyExc_IndexError,
					"index out of range");
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
		}
		else repeat |= x;
		if (revcount >= capacity) {
			PyErr_Format(PyExc_OverflowError,
				     "bitset size (%d) > capacity (%d)",
				     revcount, capacity);
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
		PyObject *obj;
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
	free(revs);
	return ret;

bail:
	free(revs);
	Py_XDECREF(ret);
	return NULL;
}

/*
 * Given a (possibly overlapping) set of revs, return the greatest
 * common ancestors: those with the longest path to the root.
 */
static PyObject *index_ancestors(indexObject *self, PyObject *args)
{
	PyObject *gca = index_commonancestorsheads(self, args);
	if (gca == NULL)
		return NULL;

	if (PyList_GET_SIZE(gca) <= 1) {
		Py_INCREF(gca);
		return gca;
	}

	return find_deepest(self, gca);
}

/*
 * Invalidate any trie entries introduced by added revs.
 */
static void nt_invalidate_added(indexObject *self, Py_ssize_t start)
{
	Py_ssize_t i, len = PyList_GET_SIZE(self->added);

	for (i = start; i < len; i++) {
		PyObject *tuple = PyList_GET_ITEM(self->added, i);
		PyObject *node = PyTuple_GET_ITEM(tuple, 7);

		nt_insert(self, PyString_AS_STRING(node), -1);
	}

	if (start == 0)
		Py_CLEAR(self->added);
}

/*
 * Delete a numeric range of revs, which must be at the end of the
 * range, but exclude the sentinel nullid entry.
 */
static int index_slice_del(indexObject *self, PyObject *item)
{
	Py_ssize_t start, stop, step, slicelength;
	Py_ssize_t length = index_length(self);
	int ret = 0;

	if (PySlice_GetIndicesEx((PySliceObject*)item, length,
				 &start, &stop, &step, &slicelength) < 0)
		return -1;

	if (slicelength <= 0)
		return 0;

	if ((step < 0 && start < stop) || (step > 0 && start > stop))
		stop = start;

	if (step < 0) {
		stop = start + 1;
		start = stop + step*(slicelength - 1) - 1;
		step = -step;
	}

	if (step != 1) {
		PyErr_SetString(PyExc_ValueError,
				"revlog index delete requires step size of 1");
		return -1;
	}

	if (stop != length - 1) {
		PyErr_SetString(PyExc_IndexError,
				"revlog index deletion indices are invalid");
		return -1;
	}

	if (start < self->length - 1) {
		if (self->nt) {
			Py_ssize_t i;

			for (i = start + 1; i < self->length - 1; i++) {
				const char *node = index_node(self, i);

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
		ret = PyList_SetSlice(self->added, start - self->length + 1,
				      PyList_GET_SIZE(self->added), NULL);
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
static int index_assign_subscript(indexObject *self, PyObject *item,
				  PyObject *value)
{
	char *node;
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
static Py_ssize_t inline_scan(indexObject *self, const char **offsets)
{
	const char *data = PyString_AS_STRING(self->data);
	Py_ssize_t pos = 0;
	Py_ssize_t end = PyString_GET_SIZE(self->data);
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

static int index_init(indexObject *self, PyObject *args)
{
	PyObject *data_obj, *inlined_obj;
	Py_ssize_t size;

	/* Initialize before argument-checking to avoid index_dealloc() crash. */
	self->raw_length = 0;
	self->added = NULL;
	self->cache = NULL;
	self->data = NULL;
	self->headrevs = NULL;
	self->filteredrevs = Py_None;
	Py_INCREF(Py_None);
	self->nt = NULL;
	self->offsets = NULL;

	if (!PyArg_ParseTuple(args, "OO", &data_obj, &inlined_obj))
		return -1;
	if (!PyString_Check(data_obj)) {
		PyErr_SetString(PyExc_TypeError, "data is not a string");
		return -1;
	}
	size = PyString_GET_SIZE(data_obj);

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

static PyObject *index_nodemap(indexObject *self)
{
	Py_INCREF(self);
	return (PyObject *)self;
}

static void index_dealloc(indexObject *self)
{
	_index_clearcaches(self);
	Py_XDECREF(self->filteredrevs);
	Py_XDECREF(self->data);
	Py_XDECREF(self->added);
	PyObject_Del(self);
}

static PySequenceMethods index_sequence_methods = {
	(lenfunc)index_length,   /* sq_length */
	0,                       /* sq_concat */
	0,                       /* sq_repeat */
	(ssizeargfunc)index_get, /* sq_item */
	0,                       /* sq_slice */
	0,                       /* sq_ass_item */
	0,                       /* sq_ass_slice */
	(objobjproc)index_contains, /* sq_contains */
};

static PyMappingMethods index_mapping_methods = {
	(lenfunc)index_length,                 /* mp_length */
	(binaryfunc)index_getitem,             /* mp_subscript */
	(objobjargproc)index_assign_subscript, /* mp_ass_subscript */
};

static PyMethodDef index_methods[] = {
	{"ancestors", (PyCFunction)index_ancestors, METH_VARARGS,
	 "return the gca set of the given revs"},
	{"commonancestorsheads", (PyCFunction)index_commonancestorsheads,
	  METH_VARARGS,
	  "return the heads of the common ancestors of the given revs"},
	{"clearcaches", (PyCFunction)index_clearcaches, METH_NOARGS,
	 "clear the index caches"},
	{"get", (PyCFunction)index_m_get, METH_VARARGS,
	 "get an index entry"},
	{"computephasesmapsets", (PyCFunction)compute_phases_map_sets,
			METH_VARARGS, "compute phases"},
	{"headrevs", (PyCFunction)index_headrevs, METH_VARARGS,
	 "get head revisions"}, /* Can do filtering since 3.2 */
	{"headrevsfiltered", (PyCFunction)index_headrevs, METH_VARARGS,
	 "get filtered head revisions"}, /* Can always do filtering */
	{"insert", (PyCFunction)index_insert, METH_VARARGS,
	 "insert an index entry"},
	{"partialmatch", (PyCFunction)index_partialmatch, METH_VARARGS,
	 "match a potentially ambiguous node ID"},
	{"stats", (PyCFunction)index_stats, METH_NOARGS,
	 "stats for the index"},
	{NULL} /* Sentinel */
};

static PyGetSetDef index_getset[] = {
	{"nodemap", (getter)index_nodemap, NULL, "nodemap", NULL},
	{NULL} /* Sentinel */
};

static PyTypeObject indexType = {
	PyObject_HEAD_INIT(NULL)
	0,                         /* ob_size */
	"parsers.index",           /* tp_name */
	sizeof(indexObject),       /* tp_basicsize */
	0,                         /* tp_itemsize */
	(destructor)index_dealloc, /* tp_dealloc */
	0,                         /* tp_print */
	0,                         /* tp_getattr */
	0,                         /* tp_setattr */
	0,                         /* tp_compare */
	0,                         /* tp_repr */
	0,                         /* tp_as_number */
	&index_sequence_methods,   /* tp_as_sequence */
	&index_mapping_methods,    /* tp_as_mapping */
	0,                         /* tp_hash */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT,        /* tp_flags */
	"revlog index",            /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	index_methods,             /* tp_methods */
	0,                         /* tp_members */
	index_getset,              /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	(initproc)index_init,      /* tp_init */
	0,                         /* tp_alloc */
};

/*
 * returns a tuple of the form (index, index, cache) with elements as
 * follows:
 *
 * index: an index object that lazily parses RevlogNG records
 * cache: if data is inlined, a tuple (index_file_content, 0), else None
 *
 * added complications are for backwards compatibility
 */
static PyObject *parse_index2(PyObject *self, PyObject *args)
{
	PyObject *tuple = NULL, *cache = NULL;
	indexObject *idx;
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

#define BUMPED_FIX 1
#define USING_SHA_256 2

static PyObject *readshas(
	const char *source, unsigned char num, Py_ssize_t hashwidth)
{
	int i;
	PyObject *list = PyTuple_New(num);
	if (list == NULL) {
		return NULL;
	}
	for (i = 0; i < num; i++) {
		PyObject *hash = PyString_FromStringAndSize(source, hashwidth);
		if (hash == NULL) {
			Py_DECREF(list);
			return NULL;
		}
		PyTuple_SetItem(list, i, hash);
		source += hashwidth;
	}
	return list;
}

static PyObject *fm1readmarker(const char *data, uint32_t *msize)
{
	const char *meta;

	double mtime;
	int16_t tz;
	uint16_t flags;
	unsigned char nsuccs, nparents, nmetadata;
	Py_ssize_t hashwidth = 20;

	PyObject *prec = NULL, *parents = NULL, *succs = NULL;
	PyObject *metadata = NULL, *ret = NULL;
	int i;

	*msize = getbe32(data);
	data += 4;
	mtime = getbefloat64(data);
	data += 8;
	tz = getbeint16(data);
	data += 2;
	flags = getbeuint16(data);
	data += 2;

	if (flags & USING_SHA_256) {
		hashwidth = 32;
	}

	nsuccs = (unsigned char)(*data++);
	nparents = (unsigned char)(*data++);
	nmetadata = (unsigned char)(*data++);

	prec = PyString_FromStringAndSize(data, hashwidth);
	data += hashwidth;
	if (prec == NULL) {
		goto bail;
	}

	succs = readshas(data, nsuccs, hashwidth);
	if (succs == NULL) {
		goto bail;
	}
	data += nsuccs * hashwidth;

	if (nparents == 1 || nparents == 2) {
		parents = readshas(data, nparents, hashwidth);
		if (parents == NULL) {
			goto bail;
		}
		data += nparents * hashwidth;
	} else {
		parents = Py_None;
	}

	meta = data + (2 * nmetadata);
	metadata = PyTuple_New(nmetadata);
	if (metadata == NULL) {
		goto bail;
	}
	for (i = 0; i < nmetadata; i++) {
		PyObject *tmp, *left = NULL, *right = NULL;
		Py_ssize_t metasize = (unsigned char)(*data++);
		left = PyString_FromStringAndSize(meta, metasize);
		meta += metasize;
		metasize = (unsigned char)(*data++);
		right = PyString_FromStringAndSize(meta, metasize);
		meta += metasize;
		if (!left || !right) {
			Py_XDECREF(left);
			Py_XDECREF(right);
			goto bail;
		}
		tmp = PyTuple_Pack(2, left, right);
		Py_DECREF(left);
		Py_DECREF(right);
		if (!tmp) {
			goto bail;
		}
		PyTuple_SetItem(metadata, i, tmp);
	}
	ret = Py_BuildValue("(OOHO(di)O)", prec, succs, flags,
			    metadata, mtime, (int)tz * 60, parents);
bail:
	Py_XDECREF(prec);
	Py_XDECREF(succs);
	Py_XDECREF(metadata);
	if (parents != Py_None)
		Py_XDECREF(parents);
	return ret;
}


static PyObject *fm1readmarkers(PyObject *self, PyObject *args) {
	const char *data;
	Py_ssize_t datalen;
	/* only unsigned long because python 2.4, should be Py_ssize_t */
	unsigned long offset, stop;
	PyObject *markers = NULL;

	/* replace kk with nn when we drop Python 2.4 */
	if (!PyArg_ParseTuple(args, "s#kk", &data, &datalen, &offset, &stop)) {
		return NULL;
	}
	data += offset;
	markers = PyList_New(0);
	if (!markers) {
		return NULL;
	}
	while (offset < stop) {
		uint32_t msize;
		int error;
		PyObject *record = fm1readmarker(data, &msize);
		if (!record) {
			goto bail;
		}
		error = PyList_Append(markers, record);
		Py_DECREF(record);
		if (error) {
			goto bail;
		}
		data += msize;
		offset += msize;
	}
	return markers;
bail:
	Py_DECREF(markers);
	return NULL;
}

static char parsers_doc[] = "Efficient content parsing.";

PyObject *encodedir(PyObject *self, PyObject *args);
PyObject *pathencode(PyObject *self, PyObject *args);
PyObject *lowerencode(PyObject *self, PyObject *args);

static PyMethodDef methods[] = {
	{"pack_dirstate", pack_dirstate, METH_VARARGS, "pack a dirstate\n"},
	{"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
	{"parse_dirstate", parse_dirstate, METH_VARARGS, "parse a dirstate\n"},
	{"parse_index2", parse_index2, METH_VARARGS, "parse a revlog index\n"},
	{"asciilower", asciilower, METH_VARARGS, "lowercase an ASCII string\n"},
	{"asciiupper", asciiupper, METH_VARARGS, "uppercase an ASCII string\n"},
	{"dict_new_presized", dict_new_presized, METH_VARARGS,
	 "construct a dict with an expected size\n"},
	{"make_file_foldmap", make_file_foldmap, METH_VARARGS,
	 "make file foldmap\n"},
	{"encodedir", encodedir, METH_VARARGS, "encodedir a path\n"},
	{"pathencode", pathencode, METH_VARARGS, "fncache-encode a path\n"},
	{"lowerencode", lowerencode, METH_VARARGS, "lower-encode a path\n"},
	{"fm1readmarkers", fm1readmarkers, METH_VARARGS,
			"parse v1 obsolete markers\n"},
	{NULL, NULL}
};

void dirs_module_init(PyObject *mod);
void manifest_module_init(PyObject *mod);

static void module_init(PyObject *mod)
{
	/* This module constant has two purposes.  First, it lets us unit test
	 * the ImportError raised without hard-coding any error text.  This
	 * means we can change the text in the future without breaking tests,
	 * even across changesets without a recompile.  Second, its presence
	 * can be used to determine whether the version-checking logic is
	 * present, which also helps in testing across changesets without a
	 * recompile.  Note that this means the pure-Python version of parsers
	 * should not have this module constant. */
	PyModule_AddStringConstant(mod, "versionerrortext", versionerrortext);

	dirs_module_init(mod);
	manifest_module_init(mod);

	indexType.tp_new = PyType_GenericNew;
	if (PyType_Ready(&indexType) < 0 ||
	    PyType_Ready(&dirstateTupleType) < 0)
		return;
	Py_INCREF(&indexType);
	PyModule_AddObject(mod, "index", (PyObject *)&indexType);
	Py_INCREF(&dirstateTupleType);
	PyModule_AddObject(mod, "dirstatetuple",
			   (PyObject *)&dirstateTupleType);

	nullentry = Py_BuildValue("iiiiiiis#", 0, 0, 0,
				  -1, -1, -1, -1, nullid, 20);
	if (nullentry)
		PyObject_GC_UnTrack(nullentry);
}

static int check_python_version(void)
{
	PyObject *sys = PyImport_ImportModule("sys"), *ver;
	long hexversion;
	if (!sys)
		return -1;
	ver = PyObject_GetAttrString(sys, "hexversion");
	Py_DECREF(sys);
	if (!ver)
		return -1;
	hexversion = PyInt_AsLong(ver);
	Py_DECREF(ver);
	/* sys.hexversion is a 32-bit number by default, so the -1 case
	 * should only occur in unusual circumstances (e.g. if sys.hexversion
	 * is manually set to an invalid value). */
	if ((hexversion == -1) || (hexversion >> 16 != PY_VERSION_HEX >> 16)) {
		PyErr_Format(PyExc_ImportError, "%s: The Mercurial extension "
			"modules were compiled with Python " PY_VERSION ", but "
			"Mercurial is currently using Python with sys.hexversion=%ld: "
			"Python %s\n at: %s", versionerrortext, hexversion,
			Py_GetVersion(), Py_GetProgramFullPath());
		return -1;
	}
	return 0;
}

#ifdef IS_PY3K
static struct PyModuleDef parsers_module = {
	PyModuleDef_HEAD_INIT,
	"parsers",
	parsers_doc,
	-1,
	methods
};

PyMODINIT_FUNC PyInit_parsers(void)
{
	PyObject *mod;

	if (check_python_version() == -1)
		return;
	mod = PyModule_Create(&parsers_module);
	module_init(mod);
	return mod;
}
#else
PyMODINIT_FUNC initparsers(void)
{
	PyObject *mod;

	if (check_python_version() == -1)
		return;
	mod = Py_InitModule3("parsers", methods, parsers_doc);
	module_init(mod);
}
#endif
