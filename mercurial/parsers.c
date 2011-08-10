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

const char nullid[20];
const int nullrev = -1;

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
static int _parse_index_ng(const char *data, int size, int inlined,
			   PyObject *index)
{
	PyObject *entry;
	int n = 0, err;
	uint64_t offset_flags;
	int comp_len, uncomp_len, base_rev, link_rev, parent_1, parent_2;
	const char *c_node_id;
	const char *end = data + size;
	uint32_t decode[8]; /* to enforce alignment with inline data */

	while (data < end) {
		unsigned int step;

		memcpy(decode, data, 32);
		offset_flags = ntohl(decode[1]);
		if (n == 0) /* mask out version number for the first entry */
			offset_flags &= 0xFFFF;
		else {
			uint32_t offset_high =  ntohl(decode[0]);
			offset_flags |= ((uint64_t)offset_high) << 32;
		}

		comp_len = ntohl(decode[2]);
		uncomp_len = ntohl(decode[3]);
		base_rev = ntohl(decode[4]);
		link_rev = ntohl(decode[5]);
		parent_1 = ntohl(decode[6]);
		parent_2 = ntohl(decode[7]);
		c_node_id = data + 32;

		entry = Py_BuildValue("Liiiiiis#", offset_flags, comp_len,
			      uncomp_len, base_rev, link_rev,
			      parent_1, parent_2, c_node_id, 20);

		if (!entry)
			return 0;

		PyObject_GC_UnTrack(entry); /* don't waste time with this */

		if (inlined) {
			err = PyList_Append(index, entry);
			Py_DECREF(entry);
			if (err)
				return 0;
		} else
			PyList_SET_ITEM(index, n, entry); /* steals reference */

		n++;
		step = 64 + (inlined ? comp_len : 0);
		if (data + step > end || data + step < data)
			break;
		data += step;
	}
	if (data != end) {
		if (!PyErr_Occurred())
			PyErr_SetString(PyExc_ValueError, "corrupt index file");
		return 0;
	}

	/* create the magic nullid entry in the index at [-1] */
	entry = Py_BuildValue("Liiiiiis#", (uint64_t)0, 0, 0, -1, -1, -1, -1, nullid, 20);

	if (!entry)
		return 0;

	PyObject_GC_UnTrack(entry); /* don't waste time with this */

	if (inlined) {
		err = PyList_Append(index, entry);
		Py_DECREF(entry);
		if (err)
			return 0;
	} else
		PyList_SET_ITEM(index, n, entry); /* steals reference */

	return 1;
}

/* This function parses a index file and returns a Python tuple of the
 * following format: (index, cache)
 *
 * index: a list of tuples containing the RevlogNG records
 * cache: if data is inlined, a tuple (index_file_content, 0) else None
 */
static PyObject *parse_index2(PyObject *self, PyObject *args)
{
	const char *data;
	int size, inlined;
	PyObject *rval = NULL, *index = NULL, *cache = NULL;
	PyObject *data_obj = NULL, *inlined_obj;

	if (!PyArg_ParseTuple(args, "s#O", &data, &size, &inlined_obj))
		return NULL;
	inlined = inlined_obj && PyObject_IsTrue(inlined_obj);

	/* If no data is inlined, we know the size of the index list in
	 * advance: size divided by the size of one revlog record (64 bytes)
	 * plus one for nullid */
	index = inlined ? PyList_New(0) : PyList_New(size / 64 + 1);
	if (!index)
		goto quit;

	/* set up the cache return value */
	if (inlined) {
		/* Note that the reference to data_obj is only borrowed */
		data_obj = PyTuple_GET_ITEM(args, 0);
		cache = Py_BuildValue("iO", 0, data_obj);
		if (!cache)
			goto quit;
	} else {
		cache = Py_None;
		Py_INCREF(Py_None);
	}

	/* actually populate the index with data */
	if (!_parse_index_ng(data, size, inlined, index))
		goto quit;

	rval = Py_BuildValue("NN", index, cache);
	if (!rval)
		goto quit;
	return rval;

quit:
	Py_XDECREF(index);
	Py_XDECREF(cache);
	Py_XDECREF(rval);
	return NULL;
}


static char parsers_doc[] = "Efficient content parsing.";

static PyMethodDef methods[] = {
	{"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
	{"parse_dirstate", parse_dirstate, METH_VARARGS, "parse a dirstate\n"},
	{"parse_index2", parse_index2, METH_VARARGS, "parse a revlog index\n"},
	{NULL, NULL}
};

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
	return PyModule_Create(&parsers_module);
}
#else
PyMODINIT_FUNC initparsers(void)
{
	Py_InitModule3("parsers", methods, parsers_doc);
}
#endif

