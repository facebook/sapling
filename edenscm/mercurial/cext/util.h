/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 util.h - utility functions for interfacing with the various python APIs.

 Copyright Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#ifndef _HG_UTIL_H_
#define _HG_UTIL_H_

#include "edenscm/mercurial/compat.h"

#if PY_MAJOR_VERSION >= 3
#define IS_PY3K
#endif

/* clang-format off */
typedef struct {
	PyObject_HEAD
	char state;
	int mode;
	int size;
	int mtime;
} dirstateTupleObject;
/* clang-format on */

extern PyTypeObject dirstateTupleType;
#define dirstate_tuple_check(op) (Py_TYPE(op) == &dirstateTupleType)

#define MIN(a, b) (((a) < (b)) ? (a) : (b))
/* VC9 doesn't include bool and lacks stdbool.h based on my searching */
#if defined(_MSC_VER) || __STDC_VERSION__ < 199901L
#define true 1
#define false 0
typedef unsigned char bool;
#else
#include <stdbool.h>
#endif

static inline PyObject* _dict_new_presized(Py_ssize_t expected_size) {
  /* _PyDict_NewPresized expects a minused parameter, but it actually
     creates a dictionary that's the nearest power of two bigger than the
     parameter. For example, with the initial minused = 1000, the
     dictionary created has size 1024. Of course in a lot of cases that
     can be greater than the maximum load factor Python's dict object
     expects (= 2/3), so as soon as we cross the threshold we'll resize
     anyway. So create a dictionary that's at least 3/2 the size. */
  return _PyDict_NewPresized(((1 + expected_size) / 2) * 3);
}

#endif /* _HG_UTIL_H_ */
