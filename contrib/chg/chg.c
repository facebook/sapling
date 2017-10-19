/*
 * A fast client for Mercurial command server
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "hgclient.h"
#include "procutil.h"
#include "util.h"

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

struct cmdserveropts {
	char sockname[PATH_MAX];
	char initsockname[PATH_MAX];
	char redirectsockname[PATH_MAX];
	size_t argsize;
	const char **args;
};

static void initcmdserveropts(struct cmdserveropts *opts) {
	memset(opts, 0, sizeof(struct cmdserveropts));
}

static void freecmdserveropts(struct cmdserveropts *opts) {
	free(opts->args);
	opts->args = NULL;
	opts->argsize = 0;
}

/*
 * Test if an argument is a sensitive flag that should be passed to the server.
 * Return 0 if not, otherwise the number of arguments starting from the current
 * one that should be passed to the server.
 */
static size_t testsensitiveflag(const char *arg)
{
	static const struct {
		const char *name;
		size_t narg;
	} flags[] = {
		{"--config", 1},
		{"--cwd", 1},
		{"--repo", 1},
		{"--repository", 1},
		{"--traceback", 0},
		{"-R", 1},
	};
	size_t i;
	for (i = 0; i < sizeof(flags) / sizeof(flags[0]); ++i) {
		size_t len = strlen(flags[i].name);
		size_t narg = flags[i].narg;
		if (memcmp(arg, flags[i].name, len) == 0) {
			if (arg[len] == '\0') {
				/* --flag (value) */
				return narg + 1;
			} else if (arg[len] == '=' && narg > 0) {
				/* --flag=value */
				return 1;
			} else if (flags[i].name[1] != '-') {
				/* short flag */
				return 1;
			}
		}
	}
	return 0;
}

/*
 * Parse argv[] and put sensitive flags to opts->args
 */
static void setcmdserverargs(struct cmdserveropts *opts,
			     int argc, const char *argv[])
{
	size_t i, step;
	opts->argsize = 0;
	for (i = 0, step = 1; i < (size_t)argc; i += step, step = 1) {
		if (!argv[i])
			continue;  /* pass clang-analyse */
		if (strcmp(argv[i], "--") == 0)
			break;
		size_t n = testsensitiveflag(argv[i]);
		if (n == 0 || i + n > (size_t)argc)
			continue;
		opts->args = reallocx(opts->args,
				      (n + opts->argsize) * sizeof(char *));
		memcpy(opts->args + opts->argsize, argv + i,
		       sizeof(char *) * n);
		opts->argsize += n;
		step = n;
	}
}

static void preparesockdir(const char *sockdir)
{
	int r;
	r = mkdir(sockdir, 0700);
	if (r < 0 && errno != EEXIST)
		abortmsgerrno("cannot create sockdir %s", sockdir);

	struct stat st;
	r = lstat(sockdir, &st);
	if (r < 0)
		abortmsgerrno("cannot stat %s", sockdir);
	if (!S_ISDIR(st.st_mode))
		abortmsg("cannot create sockdir %s (file exists)", sockdir);
	if (st.st_uid != geteuid() || st.st_mode & 0077)
		abortmsg("insecure sockdir %s", sockdir);
}

/*
 * Check if a socket directory exists and is only owned by the current user.
 * Return 1 if so, 0 if not. This is used to check if XDG_RUNTIME_DIR can be
 * used or not. According to the specification [1], XDG_RUNTIME_DIR should be
 * ignored if the directory is not owned by the user with mode 0700.
 * [1]: https://standards.freedesktop.org/basedir-spec/basedir-spec-latest.html
 */
static int checkruntimedir(const char *sockdir)
{
	struct stat st;
	int r = lstat(sockdir, &st);
	if (r < 0) /* ex. does not exist */
		return 0;
	if (!S_ISDIR(st.st_mode)) /* ex. is a file, not a directory */
		return 0;
	return st.st_uid == geteuid() && (st.st_mode & 0777) == 0700;
}

static void getdefaultsockdir(char sockdir[], size_t size)
{
	/* by default, put socket file in secure directory
	 * (${XDG_RUNTIME_DIR}/chg, or /${TMPDIR:-tmp}/chg$UID)
	 * (permission of socket file may be ignored on some Unices) */
	const char *runtimedir = getenv("XDG_RUNTIME_DIR");
	int r;
	if (runtimedir && checkruntimedir(runtimedir)) {
		r = snprintf(sockdir, size, "%s/chg", runtimedir);
	} else {
		const char *tmpdir = getenv("TMPDIR");
		if (!tmpdir)
			tmpdir = "/tmp";
		r = snprintf(sockdir, size, "%s/chg%d", tmpdir, geteuid());
	}
	if (r < 0 || (size_t)r >= size)
		abortmsg("too long TMPDIR (r = %d)", r);
}

static void setcmdserveropts(struct cmdserveropts *opts)
{
	int r;
	char sockdir[PATH_MAX];
	const char *envsockname = getenv("CHGSOCKNAME");
	if (!envsockname) {
		getdefaultsockdir(sockdir, sizeof(sockdir));
		preparesockdir(sockdir);
	}

	const char *basename = (envsockname) ? envsockname : sockdir;
	const char *sockfmt = (envsockname) ? "%s" : "%s/server";
	r = snprintf(opts->sockname, sizeof(opts->sockname), sockfmt, basename);
	if (r < 0 || (size_t)r >= sizeof(opts->sockname))
		abortmsg("too long TMPDIR or CHGSOCKNAME (r = %d)", r);
	r = snprintf(opts->initsockname, sizeof(opts->initsockname),
			"%s.%u", opts->sockname, (unsigned)getpid());
	if (r < 0 || (size_t)r >= sizeof(opts->initsockname))
		abortmsg("too long TMPDIR or CHGSOCKNAME (r = %d)", r);
}

static const char *gethgcmd(void)
{
	static const char *hgcmd = NULL;
	if (!hgcmd) {
		hgcmd = getenv("CHGHG");
		if (!hgcmd || hgcmd[0] == '\0')
			hgcmd = getenv("HG");
		if (!hgcmd || hgcmd[0] == '\0')
#ifdef HGPATH
			hgcmd = (HGPATH);
#else
			hgcmd = "hg";
#endif
	}
	return hgcmd;
}

static void execcmdserver(const struct cmdserveropts *opts)
{
	const char *hgcmd = gethgcmd();

	const char *baseargv[] = {
		hgcmd,
		"serve",
		"--cmdserver", "chgunix",
		"--address", opts->initsockname,
		"--daemon-postexec", "chdir:/",
	};
	size_t baseargvsize = sizeof(baseargv) / sizeof(baseargv[0]);
	size_t argsize = baseargvsize + opts->argsize + 1;

	const char **argv = mallocx(sizeof(char *) * argsize);
	memcpy(argv, baseargv, sizeof(baseargv));
	memcpy(argv + baseargvsize, opts->args, sizeof(char *) * opts->argsize);
	argv[argsize - 1] = NULL;

	if (putenv("CHGINTERNALMARK=") != 0)
		abortmsgerrno("failed to putenv");
	if (execvp(hgcmd, (char **)argv) < 0)
		abortmsgerrno("failed to exec cmdserver");
	free(argv);
}

/* Retry until we can connect to the server. Give up after some time. */
static hgclient_t *retryconnectcmdserver(struct cmdserveropts *opts, pid_t pid)
{
	static const struct timespec sleepreq = {0, 10 * 1000000};
	int pst = 0;

	debugmsg("try connect to %s repeatedly", opts->initsockname);

	unsigned int timeoutsec = 60;  /* default: 60 seconds */
	const char *timeoutenv = getenv("CHGTIMEOUT");
	if (timeoutenv)
		sscanf(timeoutenv, "%u", &timeoutsec);

	for (unsigned int i = 0; !timeoutsec || i < timeoutsec * 100; i++) {
		hgclient_t *hgc = hgc_open(opts->initsockname);
		if (hgc) {
			debugmsg("rename %s to %s", opts->initsockname,
					opts->sockname);
			int r = rename(opts->initsockname, opts->sockname);
			if (r != 0)
				abortmsgerrno("cannot rename");
			return hgc;
		}

		if (pid > 0) {
			/* collect zombie if child process fails to start */
			int r = waitpid(pid, &pst, WNOHANG);
			if (r != 0)
				goto cleanup;
		}

		nanosleep(&sleepreq, NULL);
	}

	abortmsg("timed out waiting for cmdserver %s", opts->initsockname);
	return NULL;

cleanup:
	if (WIFEXITED(pst)) {
		if (WEXITSTATUS(pst) == 0)
			abortmsg("could not connect to cmdserver "
				 "(exited with status 0)");
		debugmsg("cmdserver exited with status %d", WEXITSTATUS(pst));
		exit(WEXITSTATUS(pst));
	} else if (WIFSIGNALED(pst)) {
		abortmsg("cmdserver killed by signal %d", WTERMSIG(pst));
	} else {
		abortmsg("error while waiting for cmdserver");
	}
	return NULL;
}

/* Connect to a cmdserver. Will start a new server on demand. */
static hgclient_t *connectcmdserver(struct cmdserveropts *opts)
{
	const char *sockname = opts->redirectsockname[0] ?
		opts->redirectsockname : opts->sockname;
	debugmsg("try connect to %s", sockname);
	hgclient_t *hgc = hgc_open(sockname);
	if (hgc)
		return hgc;

	/* prevent us from being connected to an outdated server: we were
	 * told by a server to redirect to opts->redirectsockname and that
	 * address does not work. we do not want to connect to the server
	 * again because it will probably tell us the same thing. */
	if (sockname == opts->redirectsockname)
		unlink(opts->sockname);

	debugmsg("start cmdserver at %s", opts->initsockname);

	pid_t pid = fork();
	if (pid < 0)
		abortmsg("failed to fork cmdserver process");
	if (pid == 0) {
		execcmdserver(opts);
	} else {
		hgc = retryconnectcmdserver(opts, pid);
	}

	return hgc;
}

static void killcmdserver(const struct cmdserveropts *opts)
{
	/* resolve config hash */
	char *resolvedpath = realpath(opts->sockname, NULL);
	if (resolvedpath) {
		unlink(resolvedpath);
		free(resolvedpath);
	}
}

/* Run instructions sent from the server like unlink and set redirect path
 * Return 1 if reconnect is needed, otherwise 0 */
static int runinstructions(struct cmdserveropts *opts, const char **insts)
{
	int needreconnect = 0;
	if (!insts)
		return needreconnect;

	assert(insts);
	opts->redirectsockname[0] = '\0';
	const char **pinst;
	for (pinst = insts; *pinst; pinst++) {
		debugmsg("instruction: %s", *pinst);
		if (strncmp(*pinst, "unlink ", 7) == 0) {
			unlink(*pinst + 7);
		} else if (strncmp(*pinst, "redirect ", 9) == 0) {
			int r = snprintf(opts->redirectsockname,
					 sizeof(opts->redirectsockname),
					 "%s", *pinst + 9);
			if (r < 0 || r >= (int)sizeof(opts->redirectsockname))
				abortmsg("redirect path is too long (%d)", r);
			needreconnect = 1;
		} else if (strncmp(*pinst, "exit ", 5) == 0) {
			int n = 0;
			if (sscanf(*pinst + 5, "%d", &n) != 1)
				abortmsg("cannot read the exit code");
			exit(n);
		} else if (strcmp(*pinst, "reconnect") == 0) {
			needreconnect = 1;
		} else {
			abortmsg("unknown instruction: %s", *pinst);
		}
	}
	return needreconnect;
}

/*
 * Test whether the command is unsupported or not. This is not designed to
 * cover all cases. But it's fast, does not depend on the server and does
 * not return false positives.
 */
static int isunsupported(int argc, const char *argv[])
{
	enum {
		SERVE = 1,
		DAEMON = 2,
		SERVEDAEMON = SERVE | DAEMON,
	};
	unsigned int state = 0;
	int i;
	for (i = 0; i < argc; ++i) {
		if (strcmp(argv[i], "--") == 0)
			break;
		if (i == 0 && strcmp("serve", argv[i]) == 0)
			state |= SERVE;
		else if (strcmp("-d", argv[i]) == 0 ||
			 strcmp("--daemon", argv[i]) == 0)
			state |= DAEMON;
	}
	return (state & SERVEDAEMON) == SERVEDAEMON;
}

static void execoriginalhg(const char *argv[])
{
	debugmsg("execute original hg");
	if (execvp(gethgcmd(), (char **)argv) < 0)
		abortmsgerrno("failed to exec original hg");
}

int main(int argc, const char *argv[], const char *envp[])
{
	if (getenv("CHGDEBUG"))
		enabledebugmsg();

	if (!getenv("HGPLAIN") && isatty(fileno(stderr)))
		enablecolor();

	if (getenv("CHGINTERNALMARK"))
		abortmsg("chg started by chg detected.\n"
			 "Please make sure ${HG:-hg} is not a symlink or "
			 "wrapper to chg. Alternatively, set $CHGHG to the "
			 "path of real hg.");

	if (isunsupported(argc - 1, argv + 1))
		execoriginalhg(argv);

	struct cmdserveropts opts;
	initcmdserveropts(&opts);
	setcmdserveropts(&opts);
	setcmdserverargs(&opts, argc, argv);

	if (argc == 2) {
		if (strcmp(argv[1], "--kill-chg-daemon") == 0) {
			killcmdserver(&opts);
			return 0;
		}
	}

	hgclient_t *hgc;
	size_t retry = 0;
	while (1) {
		hgc = connectcmdserver(&opts);
		if (!hgc)
			abortmsg("cannot open hg client");
		hgc_setenv(hgc, envp);
		const char **insts = hgc_validate(hgc, argv + 1, argc - 1);
		int needreconnect = runinstructions(&opts, insts);
		free(insts);
		if (!needreconnect)
			break;
		hgc_close(hgc);
		if (++retry > 10)
			abortmsg("too many redirections.\n"
				 "Please make sure %s is not a wrapper which "
				 "changes sensitive environment variables "
				 "before executing hg. If you have to use a "
				 "wrapper, wrap chg instead of hg.",
				 gethgcmd());
	}

	setupsignalhandler(hgc_peerpid(hgc), hgc_peerpgid(hgc));
	atexit(waitpager);
	int exitcode = hgc_runcommand(hgc, argv + 1, argc - 1);
	restoresignalhandler();
	hgc_close(hgc);
	freecmdserveropts(&opts);

	return exitcode;
}
