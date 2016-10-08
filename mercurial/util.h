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
/* The mapping of Python types is meant to be temporary to get Python
 * 3 to compile. We should remove this once Python 3 support is fully
 * supported and proper types are used in the extensions themselves. */
#define PyInt_Type PyLong_Type
#define PyInt_FromLong PyLong_FromLong
#define PyInt_AsLong PyLong_AsLong

/*
 Mapping of some of the python < 2.x PyString* functions to py3k's PyUnicode.

 The commented names below represent those that are present in the PyBytes
 definitions for python < 2.6 (below in this file) that don't have a direct
 implementation.
*/

#define PyStringObject PyUnicodeObject
#define PyString_Type PyUnicode_Type

#define PyString_Check PyUnicode_Check
#define PyString_CheckExact PyUnicode_CheckExact
#define PyString_CHECK_INTERNED PyUnicode_CHECK_INTERNED
#define PyString_AS_STRING PyUnicode_AsLatin1String
#define PyString_GET_SIZE PyUnicode_GET_SIZE

#define PyString_FromStringAndSize PyUnicode_FromStringAndSize
#define PyString_FromString PyUnicode_FromString
#define PyString_FromFormatV PyUnicode_FromFormatV
#define PyString_FromFormat PyUnicode_FromFormat
/* #define PyString_Size PyUnicode_GET_SIZE */
/* #define PyString_AsString */
/* #define PyString_Repr */
#define PyString_Concat PyUnicode_Concat
#define PyString_ConcatAndDel PyUnicode_AppendAndDel
#define _PyString_Resize PyUnicode_Resize
/* #define _PyString_Eq */
#define PyString_Format PyUnicode_Format
/* #define _PyString_FormatLong */
/* #define PyString_DecodeEscape */
#define _PyString_Join PyUnicode_Join
#define PyString_Decode PyUnicode_Decode
#define PyString_Encode PyUnicode_Encode
#define PyString_AsEncodedObject PyUnicode_AsEncodedObject
#define PyString_AsEncodedString PyUnicode_AsEncodedString
#define PyString_AsDecodedObject PyUnicode_AsDecodedObject
#define PyString_AsDecodedString PyUnicode_AsDecodedUnicode
/* #define PyString_AsStringAndSize */
#define _PyString_InsertThousandsGrouping _PyUnicode_InsertThousandsGrouping

#endif /* PY_MAJOR_VERSION */

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

#endif /* _HG_UTIL_H_ */
