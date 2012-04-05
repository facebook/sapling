/*
 parsers.c - efficient content parsing

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <ctype.h>
#include <string.h>

#include "util.h"

static int hexdigit(char c)
{
	if (c >= '0' && c <= '9')
		return c - '0';
	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;

	PyErr_SetString(PyExc_ValueError, "input contains non-hex character");
	return 0;
}

/*
 * Turn a hex-encoded string into binary.
 */
static PyObject *unhexlify(const char *str, int len)
{
	PyObject *ret;
	const char *c;
	char *d;

	ret = PyBytes_FromStringAndSize(NULL, len / 2);

	if (!ret)
		return NULL;

	d = PyBytes_AsString(ret);

	for (c = str; c < str + len;) {
		int hi = hexdigit(*c++);
		int lo = hexdigit(*c++);
		*d++ = (hi << 4) | lo;
	}

	return ret;
}

/*
 * This code assumes that a manifest is stitched together with newline
 * ('\n') characters.
 */
static PyObject *parse_manifest(PyObject *self, PyObject *args)
{
	PyObject *mfdict, *fdict;
	char *str, *cur, *start, *zero;
	int len;

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_manifest",
			      &PyDict_Type, &mfdict,
			      &PyDict_Type, &fdict,
			      &str, &len))
		goto quit;

	for (start = cur = str, zero = NULL; cur < str + len; cur++) {
		PyObject *file = NULL, *node = NULL;
		PyObject *flags = NULL;
		int nlen;

		if (!*cur) {
			zero = cur;
			continue;
		}
		else if (*cur != '\n')
			continue;

		if (!zero) {
			PyErr_SetString(PyExc_ValueError,
					"manifest entry has no separator");
			goto quit;
		}

		file = PyBytes_FromStringAndSize(start, zero - start);

		if (!file)
			goto bail;

		nlen = cur - zero - 1;

		node = unhexlify(zero + 1, nlen > 40 ? 40 : nlen);
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

		start = cur + 1;
		zero = NULL;

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

	if (len > 0 && *(cur - 1) != '\n') {
		PyErr_SetString(PyExc_ValueError,
				"manifest contains trailing garbage");
		goto quit;
	}

	Py_INCREF(Py_None);
	return Py_None;
quit:
	return NULL;
}

#ifdef _WIN32
#ifdef _MSC_VER
/* msvc 6.0 has problems */
#define inline __inline
typedef unsigned long uint32_t;
typedef unsigned __int64 uint64_t;
#else
#include <stdint.h>
#endif
static uint32_t ntohl(uint32_t x)
{
	return ((x & 0x000000ffUL) << 24) |
	       ((x & 0x0000ff00UL) <<  8) |
	       ((x & 0x00ff0000UL) >>  8) |
	       ((x & 0xff000000UL) >> 24);
}
#else
/* not windows */
#include <sys/types.h>
#if defined __BEOS__ && !defined __HAIKU__
#include <ByteOrder.h>
#else
#include <arpa/inet.h>
#endif
#include <inttypes.h>
#endif

static PyObject *parse_dirstate(PyObject *self, PyObject *args)
{
	PyObject *dmap, *cmap, *parents = NULL, *ret = NULL;
	PyObject *fname = NULL, *cname = NULL, *entry = NULL;
	char *str, *cur, *end, *cpos;
	int state, mode, size, mtime;
	unsigned int flen;
	int len;
	uint32_t decode[4]; /* for alignment */

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_dirstate",
			      &PyDict_Type, &dmap,
			      &PyDict_Type, &cmap,
			      &str, &len))
		goto quit;

	/* read parents */
	if (len < 40)
		goto quit;

	parents = Py_BuildValue("s#s#", str, 20, str + 20, 20);
	if (!parents)
		goto quit;

	/* read filenames */
	cur = str + 40;
	end = str + len;

	while (cur < end - 17) {
		/* unpack header */
		state = *cur;
		memcpy(decode, cur + 1, 16);
		mode = ntohl(decode[0]);
		size = ntohl(decode[1]);
		mtime = ntohl(decode[2]);
		flen = ntohl(decode[3]);
		cur += 17;
		if (cur + flen > end || cur + flen < cur) {
			PyErr_SetString(PyExc_ValueError, "overflow in dirstate");
			goto quit;
		}

		entry = Py_BuildValue("ciii", state, mode, size, mtime);
		if (!entry)
			goto quit;
		PyObject_GC_UnTrack(entry); /* don't waste time with this */

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
		cur += flen;
		Py_DECREF(fname);
		Py_DECREF(entry);
		fname = cname = entry = NULL;
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
 * A list-like object that decodes the contents of a RevlogNG index
 * file on demand. It has limited support for insert and delete at the
 * last element before the end.  The last entry is always a sentinel
 * nullid.
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
	int inlined;
} indexObject;

static Py_ssize_t index_length(indexObject *self)
{
	if (self->added == NULL)
		return self->length;
	return self->length + PyList_GET_SIZE(self->added);
}

static PyObject *nullentry;

static long inline_scan(indexObject *self, const char **offsets);

#if LONG_MAX == 0x7fffffffL
static const char *tuple_format = "Kiiiiiis#";
#else
static const char *tuple_format = "kiiiiiis#";
#endif

/* RevlogNG format (all in big endian, data may be inlined):
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
	uint32_t decode[8]; /* to enforce alignment with inline data */
	uint64_t offset_flags;
	int comp_len, uncomp_len, base_rev, link_rev, parent_1, parent_2;
	const char *c_node_id;
	const char *data;
	Py_ssize_t length = index_length(self);
	PyObject *entry;

	if (pos >= length) {
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

	if (self->inlined && pos > 0) {
		if (self->offsets == NULL) {
			self->offsets = malloc(self->raw_length *
					       sizeof(*self->offsets));
			if (self->offsets == NULL)
				return PyErr_NoMemory();
			inline_scan(self, self->offsets);
		}
		data = self->offsets[pos];
	} else
		data = PyString_AS_STRING(self->data) + pos * 64;

	memcpy(decode, data, 8 * sizeof(uint32_t));

	offset_flags = ntohl(decode[1]);
	if (pos == 0) /* mask out version number for the first entry */
		offset_flags &= 0xFFFF;
	else {
		uint32_t offset_high = ntohl(decode[0]);
		offset_flags |= ((uint64_t)offset_high) << 32;
	}

	comp_len = ntohl(decode[2]);
	uncomp_len = ntohl(decode[3]);
	base_rev = ntohl(decode[4]);
	link_rev = ntohl(decode[5]);
	parent_1 = ntohl(decode[6]);
	parent_2 = ntohl(decode[7]);
	c_node_id = data + 32;

	entry = Py_BuildValue(tuple_format, offset_flags, comp_len,
			      uncomp_len, base_rev, link_rev,
			      parent_1, parent_2, c_node_id, 20);

	if (entry)
		PyObject_GC_UnTrack(entry);

	self->cache[pos] = entry;
	Py_INCREF(entry);

	return entry;
}

static PyObject *index_insert(indexObject *self, PyObject *args)
{
	PyObject *obj, *node;
	long offset;
	Py_ssize_t len;

	if (!PyArg_ParseTuple(args, "lO", &offset, &obj))
		return NULL;

	if (!PyTuple_Check(obj) || PyTuple_GET_SIZE(obj) != 8) {
		PyErr_SetString(PyExc_ValueError, "8-tuple required");
		return NULL;
	}

	node = PyTuple_GET_ITEM(obj, 7);
	if (!PyString_Check(node) || PyString_GET_SIZE(node) != 20) {
		PyErr_SetString(PyExc_ValueError,
				"20-byte hash required as last element");
		return NULL;
	}

	len = index_length(self);

	if (offset < 0)
		offset += len;

	if (offset != len - 1) {
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

	Py_RETURN_NONE;
}

static int index_assign_subscript(indexObject *self, PyObject *item,
				  PyObject *value)
{
	Py_ssize_t start, stop, step, slicelength;
	Py_ssize_t length = index_length(self);

	if (!PySlice_Check(item) || value != NULL) {
		PyErr_SetString(PyExc_TypeError,
				"revlog index only supports slice deletion");
		return -1;
	}

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

	if (start < self->length) {
		self->length = start + 1;
		if (self->added) {
			Py_DECREF(self->added);
			self->added = NULL;
		}
		return 0;
	}

	return PyList_SetSlice(self->added, start - self->length + 1,
			       PyList_GET_SIZE(self->added),
			       NULL);
}

static long inline_scan(indexObject *self, const char **offsets)
{
	const char *data = PyString_AS_STRING(self->data);
	const char *end = data + PyString_GET_SIZE(self->data);
	const long hdrsize = 64;
	long incr = hdrsize;
	Py_ssize_t len = 0;

	while (data + hdrsize <= end) {
		uint32_t comp_len;
		const char *old_data;
		/* 3rd element of header is length of compressed inline data */
		memcpy(&comp_len, data + 8, sizeof(uint32_t));
		incr = hdrsize + ntohl(comp_len);
		if (incr < hdrsize)
			break;
		if (offsets)
			offsets[len] = data;
		len++;
		old_data = data;
		data += incr;
		if (data <= old_data)
			break;
	}

	if (data != end && data + hdrsize != end) {
		if (!PyErr_Occurred())
			PyErr_SetString(PyExc_ValueError, "corrupt index file");
		return -1;
	}

	return len;
}

static int index_real_init(indexObject *self, const char *data, int size,
			   PyObject *inlined_obj, PyObject *data_obj)
{
	self->inlined = inlined_obj && PyObject_IsTrue(inlined_obj);
	self->data = data_obj;
	self->cache = NULL;

	self->added = NULL;
	self->offsets = NULL;
	Py_INCREF(self->data);

	if (self->inlined) {
		long len = inline_scan(self, NULL);
		if (len == -1)
			goto bail;
		self->raw_length = len;
		self->length = len + 1;
	} else {
		if (size % 64) {
			PyErr_SetString(PyExc_ValueError, "corrupt index file");
			goto bail;
		}
		self->raw_length = size / 64;
		self->length = self->raw_length + 1;
	}

	return 0;
bail:
	return -1;
}

static int index_init(indexObject *self, PyObject *args, PyObject *kwds)
{
	const char *data;
	int size;
	PyObject *inlined_obj;

	if (!PyArg_ParseTuple(args, "s#O", &data, &size, &inlined_obj))
		return -1;

	return index_real_init(self, data, size, inlined_obj,
			       PyTuple_GET_ITEM(args, 0));
}

static void index_dealloc(indexObject *self)
{
	Py_DECREF(self->data);
	if (self->cache) {
		Py_ssize_t i;

		for (i = 0; i < self->raw_length; i++)
			Py_XDECREF(self->cache[i]);
	}
	Py_XDECREF(self->added);
	free(self->offsets);
	PyObject_Del(self);
}

static PySequenceMethods index_sequence_methods = {
	(lenfunc)index_length,   /* sq_length */
	0,                       /* sq_concat */
	0,                       /* sq_repeat */
	(ssizeargfunc)index_get, /* sq_item */
};

static PyMappingMethods index_mapping_methods = {
	(lenfunc)index_length,                 /* mp_length */
	NULL,                                  /* mp_subscript */
	(objobjargproc)index_assign_subscript, /* mp_ass_subscript */
};

static PyMethodDef index_methods[] = {
	{"insert", (PyCFunction)index_insert, METH_VARARGS,
	 "insert an index entry"},
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
	0,                         /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	(initproc)index_init,      /* tp_init */
	0,                         /* tp_alloc */
	PyType_GenericNew,         /* tp_new */
};

/*
 * returns a tuple of the form (index, None, cache) with elements as
 * follows:
 *
 * index: an index object that lazily parses the RevlogNG records
 * cache: if data is inlined, a tuple (index_file_content, 0), else None
 *
 * added complications are for backwards compatibility
 */
static PyObject *parse_index2(PyObject *self, PyObject *args)
{
	const char *data;
	int size, ret;
	PyObject *inlined_obj, *tuple = NULL, *cache = NULL;
	indexObject *idx;

	if (!PyArg_ParseTuple(args, "s#O", &data, &size, &inlined_obj))
		return NULL;

	idx = PyObject_New(indexObject, &indexType);

	if (idx == NULL)
		goto bail;

	ret = index_real_init(idx, data, size, inlined_obj,
			      PyTuple_GET_ITEM(args, 0));
	if (ret)
		goto bail;

	if (idx->inlined) {
		Py_INCREF(idx->data);
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

static char parsers_doc[] = "Efficient content parsing.";

static PyMethodDef methods[] = {
	{"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
	{"parse_dirstate", parse_dirstate, METH_VARARGS, "parse a dirstate\n"},
	{"parse_index2", parse_index2, METH_VARARGS, "parse a revlog index\n"},
	{NULL, NULL}
};

static void module_init(PyObject *mod)
{
	static const char nullid[20];

	if (PyType_Ready(&indexType) < 0)
		return;
	Py_INCREF(&indexType);

	PyModule_AddObject(mod, "index", (PyObject *)&indexType);

	nullentry = Py_BuildValue("iiiiiiis#", 0, 0, 0,
				  -1, -1, -1, -1, nullid, 20);
	if (nullentry)
		PyObject_GC_UnTrack(nullentry);
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
	PyObject *mod = PyModule_Create(&parsers_module);
	module_init(mod);
	return mod;
}
#else
PyMODINIT_FUNC initparsers(void)
{
	PyObject *mod = Py_InitModule3("parsers", methods, parsers_doc);
	module_init(mod);
}
#endif

