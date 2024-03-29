/*
 * Utility functions
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#ifndef UTIL_H_
#define UTIL_H_

#ifdef __GNUC__
#define PRINTF_FORMAT_ __attribute__((format(printf, 1, 2)))
#define UNUSED_ __attribute__((unused))
#else
#define PRINTF_FORMAT_
#define UNUSED_
#endif

void abortmsg(const char* fmt, ...) PRINTF_FORMAT_;
void abortmsgerrno(const char* fmt, ...) PRINTF_FORMAT_;

void enablecolor(void);
void enabledebugmsg(void);
void debugmsg(const char* fmt, ...) PRINTF_FORMAT_;

void fchdirx(int dirfd);
void fsetcloexec(int fd);
void* chg_mallocx(size_t size);
void* chg_reallocx(void* ptr, size_t size);
void* chg_callocx(size_t count, size_t size);

double chg_now();

int runshellcmd(const char* cmd, const char* envp[], const char* cwd);

#endif /* UTIL_H_ */
