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

