/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Copyright Matt Mackall <mpm@selenic.com> and others
 *
 * This software may be used and distributed according to the terms of
 * the GNU General Public License, incorporated herein by reference.
 */

#ifndef _HG_COMPAT_H_
#define _HG_COMPAT_H_

#ifdef _WIN32
#ifdef _MSC_VER
/* msvc 6.0 has problems */
#define inline __inline
#if defined(_WIN64)
typedef __int64 ssize_t;
typedef unsigned __int64 uintptr_t;
#else /* if defined(_WIN64) */
typedef int ssize_t;
typedef unsigned int uintptr_t;
#endif /* if defined(_WIN64) */
#if _MSC_VER < 1600
typedef signed char int8_t;
typedef short int16_t;
typedef long int32_t;
typedef __int64 int64_t;
typedef unsigned char uint8_t;
typedef unsigned short uint16_t;
typedef unsigned long uint32_t;
typedef unsigned __int64 uint64_t;
#endif /* if _MSC_VER < 1600 */
#include <stdint.h>
#endif /* ifdef _MSC_VER */
#else /* ifdef _WIN32 */
/* not windows */
#include <sys/types.h>
#if defined __BEOS__ && !defined __HAIKU__
#include <ByteOrder.h>
#else
#include <arpa/inet.h>
#endif
#include <inttypes.h>
#endif /* ifdef _WIN32 */

#if defined __hpux || defined __SUNPRO_C || defined _AIX
#define inline
#endif /* if defined __hpux || defined __SUNPRO_C || defined _AIX */

#ifdef __linux
#define inline __inline
#endif /* ifdef __linux */

#endif /* ifndef _HG_COMPAT_H_ */
