/*
 charencode.c - miscellaneous character encoding

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include "charencode.h"
#include "util.h"

#ifdef IS_PY3K
/* The mapping of Python types is meant to be temporary to get Python
 * 3 to compile. We should remove this once Python 3 support is fully
 * supported and proper types are used in the extensions themselves. */
#define PyInt_Type PyLong_Type
#define PyInt_AS_LONG PyLong_AS_LONG
#endif

static const char lowertable[128] = {
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

static const char uppertable[128] = {
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

/*
 * Turn a hex-encoded string into binary.
 */
PyObject *unhexlify(const char *str, Py_ssize_t len)
{
	PyObject *ret;
	char *d;
	Py_ssize_t i;

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

PyObject *asciilower(PyObject *self, PyObject *args)
{
	PyObject *str_obj;
	if (!PyArg_ParseTuple(args, "O!:asciilower", &PyBytes_Type, &str_obj))
		return NULL;
	return _asciitransform(str_obj, lowertable, NULL);
}

PyObject *asciiupper(PyObject *self, PyObject *args)
{
	PyObject *str_obj;
	if (!PyArg_ParseTuple(args, "O!:asciiupper", &PyBytes_Type, &str_obj))
		return NULL;
	return _asciitransform(str_obj, uppertable, NULL);
}

PyObject *make_file_foldmap(PyObject *self, PyObject *args)
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
			if (PyDict_SetItem(file_foldmap, normed, k) == -1) {
				Py_DECREF(normed);
				goto quit;
			}
			Py_DECREF(normed);
		}
	}
	return file_foldmap;
quit:
	Py_XDECREF(file_foldmap);
	return NULL;
}
