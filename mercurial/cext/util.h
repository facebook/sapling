/*
 util.h - utility functions for interfacing with the various python APIs.

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#ifndef _HG_UTIL_H_
#define _HG_UTIL_H_

#include "compat.h"

#if PY_MAJOR_VERSION >= 3
#define IS_PY3K
#endif

typedef struct {
	PyObject_HEAD
	char state;
	int mode;
	int size;
	int mtime;
} dirstateTupleObject;

extern PyTypeObject dirstateTupleType;
#define dirstate_tuple_check(op) (Py_TYPE(op) == &dirstateTupleType)

/* This should be kept in sync with normcasespecs in encoding.py. */
enum normcase_spec {
	NORMCASE_LOWER = -1,
	NORMCASE_UPPER = 1,
	NORMCASE_OTHER = 0
};

#define MIN(a, b) (((a)<(b))?(a):(b))
/* VC9 doesn't include bool and lacks stdbool.h based on my searching */
#if defined(_MSC_VER) || __STDC_VERSION__ < 199901L
#define true 1
#define false 0
typedef unsigned char bool;
#else
#include <stdbool.h>
#endif

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

static const int8_t hextable[256] = {
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

static inline int hexdigit(const char *p, Py_ssize_t off)
{
	int8_t val = hextable[(unsigned char)p[off]];

	if (val >= 0) {
		return val;
	}

	PyErr_SetString(PyExc_ValueError, "input contains non-hex character");
	return 0;
}

#endif /* _HG_UTIL_H_ */
