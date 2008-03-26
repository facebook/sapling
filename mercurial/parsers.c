/*
 parsers.c - efficient content parsing

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <ctype.h>
#include <string.h>

static int hexdigit(char c)
{
	if (c >= '0' && c <= '9')
		return c - '0';

	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;

	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	
	return -1;
}

/*
 * Turn a hex-encoded string into binary.
 */
static PyObject *unhexlify(const char *str, int len)
{
	PyObject *ret = NULL;
	char *c, *d;

	if (len % 2) {
		PyErr_SetString(PyExc_ValueError,
				"input is not even in length");
		goto bail;
	}

	ret = PyString_FromStringAndSize(NULL, len / 2);
	if (!ret)
		goto bail;

	d = PyString_AsString(ret);
	if (!d)
		goto bail;

	for (c = str; c < str + len;) {
		int hi = hexdigit(*c++);
		int lo = hexdigit(*c++);

		if (hi == -1 || lo == -1) {
			PyErr_SetString(PyExc_ValueError,
					"input contains non-hex character");
			goto bail;
		}

		*d++ = (hi << 4) | lo;
	}
	
	goto done;
	
bail:
	Py_XDECREF(ret);
	ret = NULL;
done:
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

		file = PyString_FromStringAndSize(start, zero - start);
		if (!file)
			goto bail;

		nlen = cur - zero - 1;

		node = unhexlify(zero + 1, nlen > 40 ? 40 : nlen);
		if (!node)
			goto bail;

		if (nlen > 40) {
			PyObject *flags;

			flags = PyString_FromStringAndSize(zero + 41,
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

static char parsers_doc[] = "Efficient content parsing.";

static PyMethodDef methods[] = {
	{"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
	{NULL, NULL}
};

PyMODINIT_FUNC initparsers(void)
{
	Py_InitModule3("parsers", methods, parsers_doc);
}
