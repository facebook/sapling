/*
 * Utility functions
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#include "util.h"

static int colorenabled = 0;

static inline void fsetcolor(FILE *fp, const char *code)
{
	if (!colorenabled)
		return;
	fprintf(fp, "\033[%sm", code);
}

static void vabortmsgerrno(int no, const char *fmt, va_list args)
{
	fsetcolor(stderr, "1;31");
	fputs("chg: abort: ", stderr);
	vfprintf(stderr, fmt, args);
	if (no != 0)
		fprintf(stderr, " (errno = %d, %s)", no, strerror(no));
	fsetcolor(stderr, "");
	fputc('\n', stderr);
	exit(255);
}

void abortmsg(const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	vabortmsgerrno(0, fmt, args);
	va_end(args);
}

void abortmsgerrno(const char *fmt, ...)
{
	int no = errno;
	va_list args;
	va_start(args, fmt);
	vabortmsgerrno(no, fmt, args);
	va_end(args);
}

static int debugmsgenabled = 0;

void enablecolor(void)
{
	colorenabled = 1;
}

void enabledebugmsg(void)
{
	debugmsgenabled = 1;
}

void debugmsg(const char *fmt, ...)
{
	if (!debugmsgenabled)
		return;

	va_list args;
	va_start(args, fmt);
	fsetcolor(stderr, "1;30");
	fputs("chg: debug: ", stderr);
	vfprintf(stderr, fmt, args);
	fsetcolor(stderr, "");
	fputc('\n', stderr);
	va_end(args);
}

void fchdirx(int dirfd)
{
	int r = fchdir(dirfd);
	if (r == -1)
		abortmsgerrno("failed to fchdir");
}

void fsetcloexec(int fd)
{
	int flags = fcntl(fd, F_GETFD);
	if (flags < 0)
		abortmsgerrno("cannot get flags of fd %d", fd);
	if (fcntl(fd, F_SETFD, flags | FD_CLOEXEC) < 0)
		abortmsgerrno("cannot set flags of fd %d", fd);
}

void *mallocx(size_t size)
{
	void *result = malloc(size);
	if (!result)
		abortmsg("failed to malloc");
	return result;
}

void *reallocx(void *ptr, size_t size)
{
	void *result = realloc(ptr, size);
	if (!result)
		abortmsg("failed to realloc");
	return result;
}

/*
 * Execute a shell command in mostly the same manner as system(), with the
 * give environment variables, after chdir to the given cwd. Returns a status
 * code compatible with the Python subprocess module.
 */
int runshellcmd(const char *cmd, const char *envp[], const char *cwd)
{
	enum { F_SIGINT = 1, F_SIGQUIT = 2, F_SIGMASK = 4, F_WAITPID = 8 };
	unsigned int doneflags = 0;
	int status = 0;
	struct sigaction newsa, oldsaint, oldsaquit;
	sigset_t oldmask;

	/* block or mask signals just as system() does */
	memset(&newsa, 0, sizeof(newsa));
	newsa.sa_handler = SIG_IGN;
	newsa.sa_flags = 0;
	if (sigemptyset(&newsa.sa_mask) < 0)
		goto done;
	if (sigaction(SIGINT, &newsa, &oldsaint) < 0)
		goto done;
	doneflags |= F_SIGINT;
	if (sigaction(SIGQUIT, &newsa, &oldsaquit) < 0)
		goto done;
	doneflags |= F_SIGQUIT;

	if (sigaddset(&newsa.sa_mask, SIGCHLD) < 0)
		goto done;
	if (sigprocmask(SIG_BLOCK, &newsa.sa_mask, &oldmask) < 0)
		goto done;
	doneflags |= F_SIGMASK;

	pid_t pid = fork();
	if (pid < 0)
		goto done;
	if (pid == 0) {
		sigaction(SIGINT, &oldsaint, NULL);
		sigaction(SIGQUIT, &oldsaquit, NULL);
		sigprocmask(SIG_SETMASK, &oldmask, NULL);
		if (cwd && chdir(cwd) < 0)
			_exit(127);
		const char *argv[] = {"sh", "-c", cmd, NULL};
		if (envp) {
			execve("/bin/sh", (char **)argv, (char **)envp);
		} else {
			execv("/bin/sh", (char **)argv);
		}
		_exit(127);
	} else {
		if (waitpid(pid, &status, 0) < 0)
			goto done;
		doneflags |= F_WAITPID;
	}

done:
	if (doneflags & F_SIGINT)
		sigaction(SIGINT, &oldsaint, NULL);
	if (doneflags & F_SIGQUIT)
		sigaction(SIGQUIT, &oldsaquit, NULL);
	if (doneflags & F_SIGMASK)
		sigprocmask(SIG_SETMASK, &oldmask, NULL);

	/* no way to report other errors, use 127 (= shell termination) */
	if (!(doneflags & F_WAITPID))
		return 127;
	if (WIFEXITED(status))
		return WEXITSTATUS(status);
	if (WIFSIGNALED(status))
		return -WTERMSIG(status);
	return 127;
}
