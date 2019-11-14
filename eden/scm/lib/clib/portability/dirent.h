// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef FBHGEXT_CLIB_PORTABILITY_DIRENT_H
#define FBHGEXT_CLIB_PORTABILITY_DIRENT_H

#if defined(_MSC_VER)
#include "folly/portability/Dirent.h"
#else
#include <dirent.h>
#endif

#endif /* FBHGEXT_CLIB_PORTABILITY_DIRENT_H */
