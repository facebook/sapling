// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef FBHGEXT_CLIB_PORTABILITY_MMAN_H
#define FBHGEXT_CLIB_PORTABILITY_MMAN_H

#if defined(_MSC_VER)
/* A fb-specific define which ensures that we use the static flavor of
 * mman-win32 */
#define MMAN_LIBRARY
#ifdef EDEN_WIN
#include "lib/third-party/mman-win32/mman.h"
#else
#include "sys/mman.h"
#endif
#else
#include <sys/mman.h>
#endif

#endif /* FBHGEXT_CLIB_PORTABILITY_MMAN_H */
