// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef PORTABILITY_UNISTD_H
#define PORTABILITY_UNISTD_H

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

#endif /* PORTABILITY_UNISTD_H */

