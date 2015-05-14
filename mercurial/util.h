/*
 util.h - utility functions for interfacing with the various python APIs.

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#ifndef _HG_UTIL_H_
#define _HG_UTIL_H_

#if PY_MAJOR_VERSION >= 3

#define IS_PY3K
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

#ifdef _WIN32
#ifdef _MSC_VER
/* msvc 6.0 has problems */
#define inline __inline
typedef signed char int8_t;
typedef short int16_t;
typedef long int32_t;
typedef __int64 int64_t;
typedef unsigned char uint8_t;
typedef unsigned short uint16_t;
typedef unsigned long uint32_t;
typedef unsigned __int64 uint64_t;
#else
#include <stdint.h>
#endif
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

#if defined __hpux || defined __SUNPRO_C || defined _AIX
#define inline
#endif

#ifdef __linux
#define inline __inline
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

static inline uint32_t getbe32(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 24) |
		(d[1] << 16) |
		(d[2] << 8) |
		(d[3]));
}

static inline int16_t getbeint16(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 8) |
		(d[1]));
}

static inline uint16_t getbeuint16(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 8) |
		(d[1]));
}

static inline void putbe32(uint32_t x, char *c)
{
	c[0] = (x >> 24) & 0xff;
	c[1] = (x >> 16) & 0xff;
	c[2] = (x >> 8) & 0xff;
	c[3] = (x) & 0xff;
}

static inline double getbefloat64(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;
	double ret;
	int i;
	uint64_t t = 0;
	for (i = 0; i < 8; i++) {
		t = (t<<8) + d[i];
	}
	memcpy(&ret, &t, sizeof(t));
	return ret;
}

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
