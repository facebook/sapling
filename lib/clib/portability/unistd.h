// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef FBHGEXT_CLIB_PORTABILITY_UNISTD_H
#define FBHGEXT_CLIB_PORTABILITY_UNISTD_H

#if defined(_MSC_VER)
#include <io.h>
/* MSVC's io.h header shows deprecation
warnings on these without underscore */
#define lseek _lseek
#define open _open
#define close _close
#else
#include <unistd.h>
#endif

#endif /* FBHGEXT_CLIB_PORTABILITY_UNISTD_H */
