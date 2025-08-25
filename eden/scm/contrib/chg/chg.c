/*
 * A fast client for Mercurial command server
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
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
  const char* cli_name;
};

static void initcmdserveropts(struct cmdserveropts* opts) {
  memset(opts, 0, sizeof(struct cmdserveropts));
}

static void preparesockdir(const char* sockdir) {
  int r;
  r = mkdir(sockdir, 0700);
  if (r < 0 && errno != EEXIST) {
    abortmsgerrno("cannot create sockdir %s", sockdir);
  }

  struct stat st;
  r = lstat(sockdir, &st);
  if (r < 0) {
    abortmsgerrno("cannot stat %s", sockdir);
  }
  if (!S_ISDIR(st.st_mode)) {
    abortmsg("cannot create sockdir %s (file exists)", sockdir);
  }
  if (st.st_uid != geteuid() || st.st_mode & 0077) {
    abortmsg("insecure sockdir %s", sockdir);
  }
}

/*
 * Check if a socket directory exists and is only owned by the current user.
 * Return 1 if so, 0 if not. This is used to check if XDG_RUNTIME_DIR can be
 * used or not. According to the specification [1], XDG_RUNTIME_DIR should be
 * ignored if the directory is not owned by the user with mode 0700.
 * [1]: https://standards.freedesktop.org/basedir-spec/basedir-spec-latest.html
 */
static int checkruntimedir(const char* sockdir) {
  struct stat st;
  int r = lstat(sockdir, &st);
  if (r < 0) { /* ex. does not exist */
    return 0;
  }
  if (!S_ISDIR(st.st_mode)) { /* ex. is a file, not a directory */
    return 0;
  }
  return st.st_uid == geteuid() && (st.st_mode & 0777) == 0700;
}

static void getdefaultsockdir(char sockdir[], size_t size) {
  /* by default, put socket file in secure directory
   * (${XDG_RUNTIME_DIR}/pfc, or /${TMPDIR:-tmp}/pfc$UID)
   * (permission of socket file may be ignored on some Unices) */
  const char* runtimedir = getenv("XDG_RUNTIME_DIR");
  int r;
  if (runtimedir && checkruntimedir(runtimedir)) {
    r = snprintf(sockdir, size, "%s/pfc", runtimedir);
  } else {
    const char* tmpdir = getenv("TMPDIR");
    if (!tmpdir) {
      tmpdir = "/tmp";
    }
    r = snprintf(sockdir, size, "%s/pfc%d", tmpdir, geteuid());
  }
  if (r < 0 || (size_t)r >= size) {
    abortmsg("too long TMPDIR (r = %d)", r);
  }
}

unsigned long long mycgroupid() {
#ifndef __linux__
  return 0;
#endif

  unsigned long long cgroup_id = 0;

  char cgroup_entry[PATH_MAX];
  FILE* cgroup_file = fopen("/proc/self/cgroup", "r");
  if (cgroup_file == NULL) {
    goto done;
  }

  size_t read = fread(cgroup_entry, 1, sizeof(cgroup_entry), cgroup_file);
  if (read <= 2 || read >= sizeof(cgroup_entry) ||
      cgroup_entry[read - 1] != '\n') {
    debugmsg("unexpected /proc/self/cgroup");
    goto done;
  }
  // Terminate string and trim newline.
  cgroup_entry[read - 1] = 0;

  // ex: cgroup_entry = 0::/muir.slice
  debugmsg("cgroup_entry = %s", cgroup_entry);

  // Check for and strip leading "0::".
  // https://docs.kernel.org/admin-guide/cgroup-v2.html
  // “/proc/$PID/cgroup” lists a process’s cgroup membership. [...]
  // The entry for cgroup v2 is always in the format “0::$PATH”
  if (strncmp(cgroup_entry, "0::", 3) != 0) {
    goto done;
  }
  const char* cgroup_name = cgroup_entry + 3;
  if (!*cgroup_name) {
    goto done;
  }

  // ex: cgroup_name = /muir.slice
  debugmsg("cgroup_name = %s", cgroup_name);

  // assume typical cgroup2 mount at /sys/fs/cgroup
  char cgroup_path[PATH_MAX];
  int r = snprintf(
      cgroup_path, sizeof(cgroup_path), "/sys/fs/cgroup%s", cgroup_name);
  if (r < 0 || (size_t)r >= sizeof(cgroup_path)) {
    goto done;
  }

  // ex: /sys/fs/cgroup/muir.slice
  debugmsg("cgroup_path = %s", cgroup_path);

  struct stat st;
  r = lstat(cgroup_path, &st);
  if (r < 0) {
    debugmsg("cgroup stat(%s) error: %s", cgroup_path, strerror(errno));
    goto done;
  }

  cgroup_id = st.st_ino;

  debugmsg("cgroup_id = %llu", cgroup_id);

done:
  if (cgroup_file != NULL) {
    fclose(cgroup_file);
  }

  return cgroup_id;
}

static void setcmdserveropts(struct cmdserveropts* opts, const char* cli_name) {
  int r;
  char sockdir[PATH_MAX];
  const char* envsockname = getenv("CHGSOCKNAME");
  if (!envsockname) {
    getdefaultsockdir(sockdir, sizeof(sockdir));
    preparesockdir(sockdir);
  }

  opts->cli_name = cli_name;

  const char* basename = (envsockname) ? envsockname : sockdir;

  unsigned long long cgroup_id = mycgroupid();

  if (cgroup_id > 0) {
    // Namespace socket with cgroup id. This prevents sl commands from jumping
    // into the "random" cgroup of the process that started the pfc server.
    const char* sockfmt = (envsockname) ? "%s-%s-%llu" : "%s/server-%s-%llu";
    r = snprintf(
        opts->sockname,
        sizeof(opts->sockname),
        sockfmt,
        basename,
        cli_name,
        cgroup_id);
  } else {
    const char* sockfmt = (envsockname) ? "%s-%s" : "%s/server-%s";
    r = snprintf(
        opts->sockname, sizeof(opts->sockname), sockfmt, basename, cli_name);
  }

  if (r < 0 || (size_t)r >= sizeof(opts->sockname)) {
    abortmsg("too long TMPDIR or CHGSOCKNAME (r = %d)", r);
  }
  r = snprintf(
      opts->initsockname,
      sizeof(opts->initsockname),
      "%s.%u",
      opts->sockname,
      (unsigned)getpid());
  if (r < 0 || (size_t)r >= sizeof(opts->initsockname)) {
    abortmsg("too long TMPDIR or CHGSOCKNAME (r = %d)", r);
  }
}

static const char* gethgcmd(const char* cli_name) {
  static const char* hgcmd = NULL;
  if (!hgcmd) {
    hgcmd = getenv("CHGHG");
    if (!hgcmd || hgcmd[0] == '\0') {
      hgcmd = getenv("HG");
    }
    if (!hgcmd || hgcmd[0] == '\0') {
#ifdef HGPATH
      hgcmd = (HGPATH);
#else
      hgcmd = cli_name;
    }
#endif
      if (!hgcmd || hgcmd[0] == '\0') {
        abortmsg("unknown cmd to execute\n");
      }
    }
    return hgcmd;
  }

  static void execcmdserver(const struct cmdserveropts* opts) {
    const char* hgcmd = gethgcmd(opts->cli_name);

    const char* argv[] = {
        hgcmd,
        "start-pfc-server",
        "--address",
        opts->initsockname,
        "--daemon-postexec",
        "chdir:/",
        NULL,
    };

    if (putenv("CHGINTERNALMARK=") != 0) {
      abortmsgerrno("failed to putenv");
    }
    if (execvp(hgcmd, (char**)argv) < 0) {
      abortmsgerrno("failed to exec cmdserver");
    }
  }

  /* Retry until we can connect to the server. Give up after some time. */
  static hgclient_t* retryconnectcmdserver(
      struct cmdserveropts * opts, pid_t pid) {
    static const struct timespec sleepreq = {0, 10 * 1000000};
    int pst = 0;

    debugmsg("try connect to %s repeatedly", opts->initsockname);

    unsigned int timeoutsec = 3600; /* default: 1 hour */
    const char* timeoutenv = getenv("CHGTIMEOUT");
    if (timeoutenv) {
      sscanf(timeoutenv, "%u", &timeoutsec);
    }

    for (unsigned int i = 0; !timeoutsec || i < timeoutsec * 100; i++) {
      hgclient_t* hgc = hgc_open(opts->initsockname);
      if (hgc) {
        debugmsg("unlink %s", opts->initsockname);
        int r = unlink(opts->initsockname);
        if (r != 0) {
          abortmsgerrno("cannot unlink");
        }
        return hgc;
      }

      if (pid > 0) {
        /* collect zombie if child process fails to start */
        int r = waitpid(pid, &pst, WNOHANG);
        if (r != 0) {
          goto cleanup;
        }
      }

      nanosleep(&sleepreq, NULL);
    }

    abortmsg("timed out waiting for cmdserver %s", opts->initsockname);
    return NULL;

  cleanup:
    if (WIFEXITED(pst)) {
      if (WEXITSTATUS(pst) == 0) {
        abortmsg(
            "could not connect to cmdserver "
            "(exited with status 0)");
      }
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
  static hgclient_t* connectcmdserver(struct cmdserveropts * opts) {
    const char* sockname =
        opts->redirectsockname[0] ? opts->redirectsockname : opts->sockname;
    debugmsg("try connect to %s", sockname);
    hgclient_t* hgc = hgc_open(sockname);
    if (hgc) {
      return hgc;
    }

    /* prevent us from being connected to an outdated server: we were
     * told by a server to redirect to opts->redirectsockname and that
     * address does not work. we do not want to connect to the server
     * again because it will probably tell us the same thing. */
    if (sockname == opts->redirectsockname) {
      unlink(opts->sockname);
    }

    debugmsg("start cmdserver at %s", opts->initsockname);

    pid_t pid = fork();
    if (pid < 0) {
      abortmsg("failed to fork cmdserver process");
    }
    if (pid == 0) {
      execcmdserver(opts);
    } else {
      hgc = retryconnectcmdserver(opts, pid);
    }

    return hgc;
  }

  static void killcmdserver(const struct cmdserveropts* opts) {
    /* resolve config hash */
    const char* sockname =
        opts->redirectsockname[0] ? opts->redirectsockname : opts->sockname;
    char* resolvedpath = realpath(sockname, NULL);
    if (resolvedpath) {
      int ret = unlink(resolvedpath);
      debugmsg("unlink(\"%s\") = %d", resolvedpath, ret);
      free(resolvedpath);
    }
  }

  /*
   * Test whether the command is unsupported or not. This is not designed to
   * cover all cases. But it's fast, does not depend on the server and does
   * not return false positives.
   */
  static int isunsupported(int argc, const char* argv[]) {
    enum {
      SERVE = 1,
      DAEMON = 2,
      SERVEDAEMON = SERVE | DAEMON,
    };
    unsigned int state = 0;
    int i;
    for (i = 0; i < argc; ++i) {
      if (strcmp(argv[i], "--") == 0) {
        break;
      }
      if (i == 0 && strcmp("serve", argv[i]) == 0) {
        state |= SERVE;
      } else if (
          strcmp("-d", argv[i]) == 0 || strcmp("--daemon", argv[i]) == 0) {
        state |= DAEMON;
      }
    }
    return (state & SERVEDAEMON) == SERVEDAEMON;
  }

  /*
   * Test whether any of the stdio fds are missing.
   */
  static int isstdiomissing() {
    return (
        fcntl(STDIN_FILENO, F_GETFD) == -1 ||
        fcntl(STDOUT_FILENO, F_GETFD) == -1 ||
        fcntl(STDERR_FILENO, F_GETFD) == -1);
  }

  static void execoriginalhg(const char* argv[], const char* cli_name) {
    debugmsg("execute original hg");
    if (execvp(gethgcmd(cli_name), (char**)argv) < 0) {
      abortmsgerrno("failed to exec original hg");
    }
  }

  static int configint(const char* name, int fallback) {
    const char* str = getenv(name);
    int value = fallback;
    if (str) {
      sscanf(str, "%d", &value);
    }
    return value;
  }

  int chg_main(
      int argc,
      const char* argv[],
      const char* envp[],
      const char* cli_name,
      uint64_t client_versionhash) {
    if (configint("CHGDEBUG", 0)) {
      enabledebugmsg();
    }

    if (!getenv("HGPLAIN") && isatty(fileno(stderr))) {
      enablecolor();
    }

    if (getenv("CHGINTERNALMARK")) {
      abortmsg(
          "chg started by chg detected.\n"
          "Please make sure ${HG:-hg} is not a symlink or "
          "wrapper to chg. Alternatively, set $CHGHG to the "
          "path of real hg.");
    }

    int fallback = 0;
    if (isunsupported(argc - 1, argv + 1)) {
      debugmsg("falling back - args unsupported");
      fallback = 1;
    } else if (nice(0) > 0 && !getenv("TESTTMP")) {
      debugmsg("falling back - nice > 0");
      fallback = 1;
    } else if (isstdiomissing()) {
      debugmsg("falling back - stdio missing");
      fallback = 1;
    }

    if (fallback) {
      // For cases when chg and original hg are the same binary,
      // we need to tell the original hg that we've already made
      // a decision to not use chg logic
      //
      // Besides, if the process has a high nice value (i.e.
      // low priority), do not start a chg server which will
      // inherit the low priority, and do not use a chg server,
      // since the user wants the process to have a lower
      // priority.
      setenv("CHGDISABLE", "1", 1);
      execoriginalhg(argv, cli_name);
    }

    struct cmdserveropts opts;
    initcmdserveropts(&opts);
    setcmdserveropts(&opts, cli_name);

    if (argc == 2) {
      if (strcmp(argv[1], "--kill-chg-daemon") == 0) {
        killcmdserver(&opts);
        return 0;
      }
    }

    hgclient_t* hgc;
    size_t retry = 0;
    while (1) {
      hgc = connectcmdserver(&opts);
      if (!hgc) {
        abortmsg("cannot open hg client");
      }
      int needreconnect = 0;
      uint64_t server_versionhash = hgc_versionhash(hgc);
      // Skip version check if there is an explicit socket path set,
      // which is used in tests.
      if (server_versionhash != client_versionhash && !getenv("CHGSOCKNAME")) {
        debugmsg(
            "version mismatch (client %" PRIu64 ", server %" PRIu64 ")",
            client_versionhash,
            server_versionhash);
        killcmdserver(&opts);
        needreconnect = 1;
      } else {
        debugmsg("version matched (%" PRIu64 ")", client_versionhash);
      }
      // If a client has a higher RLIMIT_NOFILE, do not reuse the existing
      // server.
      unsigned long nofile = hgc_nofile(hgc);
      if (nofile > 0) {
        struct rlimit lim = {0, 0};
        int r = getrlimit(RLIMIT_NOFILE, &lim);
        if (r != 0) {
          abortmsgerrno("cannot getrlimit");
        }
        unsigned long cur = (unsigned long)lim.rlim_cur;
        if (cur > nofile) {
          debugmsg(
              "RLIMIT_NOFILE incompatible (client %lu > server %lu)",
              cur,
              nofile);
          killcmdserver(&opts);
          needreconnect = 1;
        } else {
          debugmsg(
              "RLIMIT_NOFILE compatible (client %lu <= server %lu)",
              cur,
              nofile);
        }
      }

      // If server's groups differ from client's, restart server. We don't want
      // to cache out-of-date permissions in the server.
      if (hgc_groups_mismatch(hgc)) {
        killcmdserver(&opts);
        needreconnect = 1;
        debugmsg("groups mismatch, reconnecting");
      } else {
        debugmsg("groups match");
      }

      if (!needreconnect) {
        hgc_setenv(hgc, envp);
      }
      if (!needreconnect) {
        break;
      }
      hgc_close(hgc);
      if (++retry > 10) {
        abortmsg(
            "too many redirections.\n"
            "Please make sure %s is not a wrapper which "
            "changes sensitive environment variables "
            "before executing hg. If you have to use a "
            "wrapper, wrap chg instead of hg.",
            gethgcmd(cli_name));
      }
    }

    setupsignalhandler(hgc_peerpid(hgc), hgc_peerpgid(hgc));
    atexit(waitpager);
    int exitcode = hgc_runcommand(hgc, argv + 1, argc - 1);
    restoresignalhandler();
    hgc_close(hgc);

    return exitcode;
  }
