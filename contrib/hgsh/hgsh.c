/*
 * hgsh.c - restricted login shell for mercurial
 *
 * Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License, incorporated herein by reference.
 *
 * this program is login shell for dedicated mercurial user account. it
 * only allows few actions:
 *
 * 1. run hg in server mode on specific repository. no other hg commands
 * are allowed. we try to verify that repo to be accessed exists under
 * given top-level directory.
 *
 * 2. (optional) forward ssh connection from firewall/gateway machine to
 * "real" mercurial host, to let users outside intranet pull and push
 * changes through firewall.
 *
 * 3. (optional) run normal shell, to allow to "su" to mercurial user, use
 * "sudo" to run programs as that user, or run cron jobs as that user.
 *
 * only tested on linux yet. patches for non-linux systems welcome.
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE /* for asprintf */
#endif

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sysexits.h>
#include <unistd.h>

/*
 * user config.
 *
 * if you see a hostname below, just use first part of hostname. example,
 * if you have host named foo.bar.com, use "foo".
 */

/*
 * HG_GATEWAY: hostname of gateway/firewall machine that people outside your
 * intranet ssh into if they need to ssh to other machines. if you do not
 * have such machine, set to NULL.
 */
#ifndef HG_GATEWAY
#define HG_GATEWAY     "gateway"
#endif

/*
 * HG_HOST: hostname of mercurial server. if any machine is allowed, set to
 * NULL.
 */
#ifndef HG_HOST
#define HG_HOST         "mercurial"
#endif

/*
 * HG_USER: username to log in from HG_GATEWAY to HG_HOST. if gateway and
 * host username are same, set to NULL.
 */
#ifndef HG_USER
#define HG_USER         "hg"
#endif

/*
 * HG_ROOT: root of tree full of mercurial repos. if you do not want to
 * validate location of repo when someone is try to access, set to NULL.
 */
#ifndef HG_ROOT
#define HG_ROOT         "/home/hg/repos"
#endif

/*
 * HG: path to the mercurial executable to run.
 */
#ifndef HG
#define HG              "/home/hg/bin/hg"
#endif

/*
 * HG_SHELL: shell to use for actions like "sudo" and "su" access to
 * mercurial user, and cron jobs. if you want to make these things
 * impossible, set to NULL.
 */
#ifndef HG_SHELL
#define HG_SHELL        NULL
/* #define HG_SHELL        "/bin/bash" */
#endif

/*
 * HG_HELP: some way for users to get support if they have problem. if they
 * should not get helpful message, set to NULL.
 */
#ifndef HG_HELP
#define HG_HELP         "please contact support@example.com for help."
#endif

/*
 * SSH: path to ssh executable to run, if forwarding from HG_GATEWAY to
 * HG_HOST. if you want to use rsh instead (why?), you need to modify
 * arguments it is called with. see forward_through_gateway.
 */
#ifndef SSH
#define SSH             "/usr/bin/ssh"
#endif

/*
 * tell whether to print command that is to be executed. useful for
 * debugging. should not interfere with mercurial operation, since
 * mercurial only cares about stdin and stdout, and this prints to stderr.
 */
static const int debug = 0;

static void print_cmdline(int argc, char **argv)
{
	FILE *fp = stderr;
	int i;

	fputs("command: ", fp);

	for (i = 0; i < argc; i++) {
		char *spc = strpbrk(argv[i], " \t\r\n");
		if (spc) {
			fputc('\'', fp);
		}
		fputs(argv[i], fp);
		if (spc) {
			fputc('\'', fp);
		}
		if (i < argc - 1) {
			fputc(' ', fp);
		}
	}
	fputc('\n', fp);
	fflush(fp);
}

static void usage(const char *reason, int exitcode)
{
	char *hg_help = HG_HELP;

	if (reason) {
		fprintf(stderr, "*** Error: %s.\n", reason);
	}
	fprintf(stderr, "*** This program has been invoked incorrectly.\n");
	if (hg_help) {
		fprintf(stderr, "*** %s\n", hg_help);
	}
	exit(exitcode ? exitcode : EX_USAGE);
}

/*
 * run on gateway host to make another ssh connection, to "real" mercurial
 * server. it sends its command line unmodified to far end.
 *
 * never called if HG_GATEWAY is NULL.
 */
static void forward_through_gateway(int argc, char **argv)
{
	char *ssh = SSH;
	char *hg_host = HG_HOST;
	char *hg_user = HG_USER;
	char **nargv = alloca((10 + argc) * sizeof(char *));
	int i = 0, j;

	nargv[i++] = ssh;
	nargv[i++] = "-q";
	nargv[i++] = "-T";
	nargv[i++] = "-x";
	if (hg_user) {
		nargv[i++] = "-l";
		nargv[i++] = hg_user;
	}
	nargv[i++] = hg_host;

	/*
	 * sshd called us with added "-c", because it thinks we are a shell.
	 * drop it if we find it.
	 */
	j = 1;
	if (j < argc && strcmp(argv[j], "-c") == 0) {
		j++;
	}

	for (; j < argc; i++, j++) {
		nargv[i] = argv[j];
	}
	nargv[i] = NULL;

	if (debug) {
		print_cmdline(i, nargv);
	}

	execv(ssh, nargv);
	perror(ssh);
	exit(EX_UNAVAILABLE);
}

/*
 * run shell. let administrator "su" to mercurial user's account to do
 * administrative works.
 *
 * never called if HG_SHELL is NULL.
 */
static void run_shell(int argc, char **argv)
{
	char *hg_shell = HG_SHELL;
	char **nargv;
	char *c;
	int i;

	nargv = alloca((argc + 3) * sizeof(char *));
	c = strrchr(hg_shell, '/');

	/* tell "real" shell it is login shell, if needed. */

	if (argv[0][0] == '-' && c) {
		nargv[0] = strdup(c);
		if (nargv[0] == NULL) {
			perror("malloc");
			exit(EX_OSERR);
		}
		nargv[0][0] = '-';
	} else {
		nargv[0] = hg_shell;
	}

	for (i = 1; i < argc; i++) {
		nargv[i] = argv[i];
	}
	nargv[i] = NULL;

	if (debug) {
		print_cmdline(i, nargv);
	}

	execv(hg_shell, nargv);
	perror(hg_shell);
	exit(EX_OSFILE);
}

enum cmdline {
	hg_init,
	hg_serve,
};


/*
 * attempt to verify that a directory is really a hg repo, by testing
 * for the existence of a subdirectory.
 */
static int validate_repo(const char *repo_root, const char *subdir)
{
	char *abs_path;
	struct stat st;
	int ret;

	if (asprintf(&abs_path, "%s.hg/%s", repo_root, subdir) == -1) {
		ret = -1;
		goto bail;
	}

	/* verify that we really are looking at valid repo. */

	if (stat(abs_path, &st) == -1) {
		ret = 0;
	} else {
		ret = 1;
	}

bail:
	return ret;
}

/*
 * paranoid wrapper, runs hg executable in server mode.
 */
static void serve_data(int argc, char **argv)
{
	char *hg_root = HG_ROOT;
	char *repo, *repo_root;
	enum cmdline cmd;
	char *nargv[6];
	size_t repolen;
	int i;

	/*
	 * check argv for looking okay. we should be invoked with argv
	 * resembling like this:
	 *
	 *   hgsh
	 *   -c
	 *   hg -R some/path serve --stdio
	 *
	 * the "-c" is added by sshd, because it thinks we are login shell.
	 */

	if (argc != 3) {
		goto badargs;
	}

	if (strcmp(argv[1], "-c") != 0) {
		goto badargs;
	}

	if (sscanf(argv[2], "hg init %as", &repo) == 1) {
		cmd = hg_init;
	}
	else if (sscanf(argv[2], "hg -R %as serve --stdio", &repo) == 1) {
		cmd = hg_serve;
	} else {
		goto badargs;
	}

	repolen = repo ? strlen(repo) : 0;

	if (repolen == 0) {
		goto badargs;
	}

	if (hg_root) {
		if (asprintf(&repo_root, "%s/%s/", hg_root, repo) == -1) {
			goto badargs;
		}

		/*
		 * attempt to stop break out from inside the
		 * repository tree. could do something more clever
		 * here, because e.g. we could traverse a symlink that
		 * looks safe, but really breaks us out of tree.
		 */

		if (strstr(repo_root, "/../") != NULL) {
			goto badargs;
		}

		/* only hg init expects no repo. */

		if (cmd != hg_init) {
			int valid;

			valid = validate_repo(repo_root, "data");

			if (valid == -1) {
				goto badargs;
			}

			if (valid == 0) {
				valid = validate_repo(repo_root, "store");

				if (valid == -1) {
					goto badargs;
				}
			}

			if (valid == 0) {
				perror(repo);
				exit(EX_DATAERR);
			}
		}

		if (chdir(hg_root) == -1) {
			perror(hg_root);
			exit(EX_SOFTWARE);
		}
	}

	i = 0;

	switch (cmd) {
	case hg_serve:
		nargv[i++] = HG;
		nargv[i++] = "-R";
		nargv[i++] = repo;
		nargv[i++] = "serve";
		nargv[i++] = "--stdio";
		break;
	case hg_init:
		nargv[i++] = HG;
		nargv[i++] = "init";
		nargv[i++] = repo;
		break;
	}

	nargv[i] = NULL;

	if (debug) {
		print_cmdline(i, nargv);
	}

	execv(HG, nargv);
	perror(HG);
	exit(EX_UNAVAILABLE);

badargs:
	/* print useless error message. */

	usage("invalid arguments", EX_DATAERR);
}

int main(int argc, char **argv)
{
	char host[1024];
	char *c;

	if (gethostname(host, sizeof(host)) == -1) {
		perror("gethostname");
		exit(EX_OSERR);
	}

	if ((c = strchr(host, '.')) != NULL) {
		*c = '\0';
	}

	if (getenv("SSH_CLIENT")) {
		char *hg_gateway = HG_GATEWAY;
		char *hg_host = HG_HOST;

		if (hg_gateway && strcmp(host, hg_gateway) == 0) {
			forward_through_gateway(argc, argv);
		}

		if (hg_host && strcmp(host, hg_host) != 0) {
			usage("invoked on unexpected host", EX_USAGE);
		}

		serve_data(argc, argv);
	} else if (HG_SHELL) {
		run_shell(argc, argv);
	} else {
		usage("invalid arguments", EX_DATAERR);
	}

	return 0;
}
