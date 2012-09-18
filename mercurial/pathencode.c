/*
 pathencode.c - efficient path name encoding

 Copyright 2012 Facebook

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <assert.h>
#include <ctype.h>
#include <stdlib.h>
#include <string.h>

#include "util.h"

/* state machine for dir-encoding */
enum dir_state {
	DDOT,
	DH,
	DHGDI,
	DDEFAULT,
};

static inline void charcopy(char *dest, Py_ssize_t *destlen, size_t destsize,
                            char c)
{
	if (dest) {
		assert(*destlen < destsize);
		dest[*destlen] = c;
	}
	(*destlen)++;
}

static inline void memcopy(char *dest, Py_ssize_t *destlen, size_t destsize,
                           const void *src, Py_ssize_t len)
{
	if (dest) {
		assert(*destlen + len < destsize);
		memcpy((void *)&dest[*destlen], src, len);
	}
	*destlen += len;
}

static Py_ssize_t _encodedir(char *dest, size_t destsize,
                             const char *src, Py_ssize_t len)
{
	enum dir_state state = DDEFAULT;
	Py_ssize_t i = 0, destlen = 0;

	while (i < len) {
		switch (state) {
		case DDOT:
			switch (src[i]) {
			case 'd':
			case 'i':
				state = DHGDI;
				charcopy(dest, &destlen, destsize, src[i++]);
				break;
			case 'h':
				state = DH;
				charcopy(dest, &destlen, destsize, src[i++]);
				break;
			default:
				state = DDEFAULT;
				break;
			}
			break;
		case DH:
			if (src[i] == 'g') {
				state = DHGDI;
				charcopy(dest, &destlen, destsize, src[i++]);
			}
			else state = DDEFAULT;
			break;
		case DHGDI:
			if (src[i] == '/') {
				memcopy(dest, &destlen, destsize, ".hg", 3);
				charcopy(dest, &destlen, destsize, src[i++]);
			}
			state = DDEFAULT;
			break;
		case DDEFAULT:
			if (src[i] == '.')
				state = DDOT;
			charcopy(dest, &destlen, destsize, src[i++]);
			break;
		}
	}

	return destlen;
}

PyObject *encodedir(PyObject *self, PyObject *args)
{
	Py_ssize_t len, newlen;
	PyObject *pathobj, *newobj;
	char *path;

	if (!PyArg_ParseTuple(args, "O:encodedir", &pathobj))
		return NULL;

	if (PyString_AsStringAndSize(pathobj, &path, &len) == -1) {
		PyErr_SetString(PyExc_TypeError, "expected a string");
		return NULL;
	}

	newlen = len ? _encodedir(NULL, 0, path, len + 1) : 1;

	if (newlen == len + 1) {
		Py_INCREF(pathobj);
		return pathobj;
	}

	newobj = PyString_FromStringAndSize(NULL, newlen);

	if (newobj) {
		PyString_GET_SIZE(newobj)--;
		_encodedir(PyString_AS_STRING(newobj), newlen, path,
			   len + 1);
	}

	return newobj;
}
