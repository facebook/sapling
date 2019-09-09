/*
 * A command server client that uses Unix domain socket
 *
 * Copyright (c) 2011 Yuya Nishihara <yuya@tcha.org>
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include <arpa/inet.h> /* for ntohl(), htonl() */
#include <assert.h>
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include "hgclient.h"
#include "procutil.h"
#include "util.h"

enum {
  CAP_GETENCODING = 0x0001,
  CAP_RUNCOMMAND = 0x0002,
  /* cHg extension: */
  CAP_ATTACHIO = 0x0100,
  CAP_CHDIR = 0x0200,
  CAP_SETENV = 0x0800,
  CAP_SETUMASK = 0x1000,
  CAP_SETPROCNAME = 0x4000,
};

typedef struct {
  const char* name;
  unsigned int flag;
} cappair_t;

static const cappair_t captable[] = {
    {"getencoding", CAP_GETENCODING},
    {"runcommand", CAP_RUNCOMMAND},
    {"attachio", CAP_ATTACHIO},
    {"chdir", CAP_CHDIR},
    {"setenv", CAP_SETENV},
    {"setumask", CAP_SETUMASK},
    {"setprocname", CAP_SETPROCNAME},
    {NULL, 0}, /* terminator */
};

typedef struct {
  char ch;
  char* data;
  size_t maxdatasize;
  size_t datasize;
} context_t;

struct hgclient_tag_ {
  int sockfd;
  pid_t pgid;
  pid_t pid;
  context_t ctx;
  unsigned int capflags;
  unsigned long long versionhash;
  double connectedat;
};

static const size_t defaultdatasize = 4096;

static void attachio(hgclient_t* hgc);

static void initcontext(context_t* ctx) {
  ctx->ch = '\0';
  ctx->data = malloc(defaultdatasize);
  ctx->maxdatasize = (ctx->data) ? defaultdatasize : 0;
  ctx->datasize = 0;
  debugmsg("initialize context buffer with size %zu", ctx->maxdatasize);
}

static void enlargecontext(context_t* ctx, size_t newsize) {
  if (newsize <= ctx->maxdatasize)
    return;

  newsize =
      defaultdatasize * ((newsize + defaultdatasize - 1) / defaultdatasize);
  ctx->data = chg_reallocx(ctx->data, newsize);
  ctx->maxdatasize = newsize;
  debugmsg("enlarge context buffer to %zu", ctx->maxdatasize);
}

static void freecontext(context_t* ctx) {
  debugmsg("free context buffer");
  free(ctx->data);
  ctx->data = NULL;
  ctx->maxdatasize = 0;
  ctx->datasize = 0;
}

/* Read channeled response from cmdserver */
static void readchannel(hgclient_t* hgc) {
  assert(hgc);

  uint32_t datasize_n;
  struct iovec iov[2] = {
      {&hgc->ctx.ch, sizeof(hgc->ctx.ch)},
      {&datasize_n, sizeof(datasize_n)},
  };
  struct msghdr msg = {};
  msg.msg_iov = iov;
  msg.msg_iovlen = 2;
  ssize_t rsize = recvmsg(hgc->sockfd, &msg, 0);
  if (rsize != sizeof(hgc->ctx.ch) + sizeof(datasize_n)) {
    /* server would have exception and traceback would be printed */
    debugmsg("failed to read channel and data size");
    exit(255);
  }

  /* datasize denotes the maximum size to write if input request */
  hgc->ctx.datasize = ntohl(datasize_n);
  enlargecontext(&hgc->ctx, hgc->ctx.datasize);

  if (isupper(hgc->ctx.ch))
    return; /* assumes input request */

  size_t cursize = 0;
  while (cursize < hgc->ctx.datasize) {
    rsize = recv(
        hgc->sockfd, hgc->ctx.data + cursize, hgc->ctx.datasize - cursize, 0);
    if (rsize < 1)
      abortmsg("failed to read data block");
    cursize += rsize;
  }
}

static void sendall(int sockfd, const void* data, size_t datasize) {
  const char* p = data;
  const char* const endp = p + datasize;
  while (p < endp) {
    ssize_t r = send(sockfd, p, endp - p, 0);
    if (r < 0)
      abortmsgerrno("cannot communicate");
    p += r;
  }
}

/* Write lengh-data block to cmdserver */
static void writeblock(const hgclient_t* hgc) {
  assert(hgc);

  const uint32_t datasize_n = htonl(hgc->ctx.datasize);
  sendall(hgc->sockfd, &datasize_n, sizeof(datasize_n));

  sendall(hgc->sockfd, hgc->ctx.data, hgc->ctx.datasize);
}

static void writeblockrequest(const hgclient_t* hgc, const char* chcmd) {
  debugmsg("request %s, block size %zu", chcmd, hgc->ctx.datasize);

  char buf[strlen(chcmd) + 1];
  memcpy(buf, chcmd, sizeof(buf) - 1);
  buf[sizeof(buf) - 1] = '\n';
  sendall(hgc->sockfd, buf, sizeof(buf));

  writeblock(hgc);
}

/* Build '\0'-separated list of args. argsize < 0 denotes that args are
 * terminated by NULL. */
static void
packcmdargs(context_t* ctx, const char* const args[], ssize_t argsize) {
  ctx->datasize = 0;
  const char* const* const end = (argsize >= 0) ? args + argsize : NULL;
  for (const char* const* it = args; it != end && *it; ++it) {
    const size_t n = strlen(*it) + 1; /* include '\0' */
    enlargecontext(ctx, ctx->datasize + n);
    memcpy(ctx->data + ctx->datasize, *it, n);
    ctx->datasize += n;
  }

  if (ctx->datasize > 0)
    --ctx->datasize; /* strip last '\0' */
}

/* Parse a series of '\0'-terminated strings from ctx-data
 * Returns an array of pointers to the start of each string */
static const char** unpackcmdargsnul(const context_t* ctx) {
  const char** args = NULL;
  size_t nargs = 0, maxnargs = 0;
  const char* s = ctx->data;
  const char* e = ctx->data + ctx->datasize;
  for (;;) {
    if (nargs + 1 >= maxnargs) { /* including last NULL */
      maxnargs += 256;
      args = chg_reallocx(args, maxnargs * sizeof(args[0]));
    }
    const char* nulbyte = memchr(s, '\0', e - s);
    if (!nulbyte)
      break;
    args[nargs] = s;
    nargs++;
    s = nulbyte + 1;
  }
  args[nargs] = NULL;
  return args;
}

static void handlereadrequest(hgclient_t* hgc) {
  context_t* ctx = &hgc->ctx;
  size_t r = fread(ctx->data, sizeof(ctx->data[0]), ctx->datasize, stdin);
  ctx->datasize = r;
  writeblock(hgc);
}

/* Read single-line */
static void handlereadlinerequest(hgclient_t* hgc) {
  context_t* ctx = &hgc->ctx;
  if (!fgets(ctx->data, ctx->datasize, stdin))
    ctx->data[0] = '\0';
  ctx->datasize = strlen(ctx->data);
  writeblock(hgc);
}

/* Execute the requested command and write exit code */
static void handlesystemrequest(hgclient_t* hgc) {
  context_t* ctx = &hgc->ctx;
  const char** args = unpackcmdargsnul(ctx);
  if (!args[0] || !args[1])
    abortmsg("missing command or cwd in system request");

  const char* cmd = args[0];
  const char* cwd = args[1];
  const char** envp = args + 2;

  debugmsg("run '%s' at '%s'", cmd, cwd);
  int32_t r = runshellcmd(cmd, envp, cwd);
  free(args);

  uint32_t r_n = htonl(r);
  memcpy(ctx->data, &r_n, sizeof(r_n));
  ctx->datasize = sizeof(r_n);
  writeblock(hgc);
}

static void handlepagerrequest(hgclient_t* hgc) {
  context_t* ctx = &hgc->ctx;
  const char** args = unpackcmdargsnul(ctx);
  if (!args[0] || !args[1])
    abortmsg("missing command or redirect settings in pager request");

  const char* cmd = args[0];
  const char* redirect = args[1];
  const char** envp = args + 2;

  setuppager(cmd, envp);
  if (hgc->capflags & CAP_ATTACHIO)
    attachio(hgc);
  /* unblock the server */
  static const char emptycmd[] = "\n";
  sendall(hgc->sockfd, emptycmd, sizeof(emptycmd) - 1);
}

/* Read response of command execution until receiving 'r'-esult */
static void handleresponse(hgclient_t* hgc) {
  for (;;) {
    readchannel(hgc);
    context_t* ctx = &hgc->ctx;
    debugmsg("response read from channel %c, size %zu", ctx->ch, ctx->datasize);
    switch (ctx->ch) {
      case 'o':
        fwrite(ctx->data, sizeof(ctx->data[0]), ctx->datasize, stdout);
        break;
      case 'e':
        fwrite(ctx->data, sizeof(ctx->data[0]), ctx->datasize, stderr);
        break;
      case 'd':
        /* assumes last char is '\n' */
        ctx->data[ctx->datasize - 1] = '\0';
        debugmsg("server: %s", ctx->data);
        break;
      case 'r':
        return;
      case 'I':
        handlereadrequest(hgc);
        break;
      case 'L':
        handlereadlinerequest(hgc);
        break;
      case 's':
        handlesystemrequest(hgc);
        break;
      case 'p':
        handlepagerrequest(hgc);
        break;
      default:
        if (isupper(ctx->ch))
          abortmsg("cannot handle response (ch = %c)", ctx->ch);
    }
  }
}

static unsigned int parsecapabilities(const char* s, const char* e) {
  unsigned int flags = 0;
  while (s < e) {
    const char* t = strchr(s, ' ');
    if (!t || t > e)
      t = e;
    const cappair_t* cap;
    for (cap = captable; cap->flag; ++cap) {
      size_t n = t - s;
      if (strncmp(s, cap->name, n) == 0 && strlen(cap->name) == n) {
        flags |= cap->flag;
        break;
      }
    }
    s = t + 1;
  }
  return flags;
}

static void readhello(hgclient_t* hgc) {
  readchannel(hgc);
  context_t* ctx = &hgc->ctx;
  if (ctx->ch != 'o') {
    char ch = ctx->ch;
    if (ch == 'e') {
      /* write early error and will exit */
      fwrite(ctx->data, sizeof(ctx->data[0]), ctx->datasize, stderr);
      handleresponse(hgc);
    }
    abortmsg("unexpected channel of hello message (ch = %c)", ch);
  }
  enlargecontext(ctx, ctx->datasize + 1);
  ctx->data[ctx->datasize] = '\0';
  debugmsg("hello received: %s (size = %zu)", ctx->data, ctx->datasize);

  const char* s = ctx->data;
  const char* const dataend = ctx->data + ctx->datasize;
  while (s < dataend) {
    const char* t = strchr(s, ':');
    if (!t || t[1] != ' ')
      break;
    const char* u = strchr(t + 2, '\n');
    if (!u)
      u = dataend;
    if (strncmp(s, "capabilities:", t - s + 1) == 0) {
      hgc->capflags = parsecapabilities(t + 2, u);
    } else if (strncmp(s, "pgid:", t - s + 1) == 0) {
      hgc->pgid = strtol(t + 2, NULL, 10);
    } else if (strncmp(s, "pid:", t - s + 1) == 0) {
      hgc->pid = strtol(t + 2, NULL, 10);
    } else if (strncmp(s, "versionhash:", t - s + 1) == 0) {
      hgc->versionhash = strtoull(t + 2, NULL, 10);
    }
    s = u + 1;
  }
  debugmsg("capflags=0x%04x, pid=%d", hgc->capflags, hgc->pid);
}

static void updateprocname(hgclient_t* hgc) {
  int r = snprintf(
      hgc->ctx.data, hgc->ctx.maxdatasize, "chg[worker/%d]", (int)getpid());
  if (r < 0 || (size_t)r >= hgc->ctx.maxdatasize)
    abortmsg("insufficient buffer to write procname (r = %d)", r);
  hgc->ctx.datasize = (size_t)r;
  writeblockrequest(hgc, "setprocname");
}

static void attachio(hgclient_t* hgc) {
  debugmsg("request attachio");
  static const char chcmd[] = "attachio\n";
  sendall(hgc->sockfd, chcmd, sizeof(chcmd) - 1);
  readchannel(hgc);
  context_t* ctx = &hgc->ctx;
  if (ctx->ch != 'I')
    abortmsg("unexpected response for attachio (ch = %c)", ctx->ch);

  static const int fds[3] = {STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO};
  struct msghdr msgh;
  memset(&msgh, 0, sizeof(msgh));
  struct iovec iov = {ctx->data, ctx->datasize}; /* dummy payload */
  msgh.msg_iov = &iov;
  msgh.msg_iovlen = 1;
  char fdbuf[CMSG_SPACE(sizeof(fds))];
  msgh.msg_control = fdbuf;
  msgh.msg_controllen = sizeof(fdbuf);
  struct cmsghdr* cmsg = CMSG_FIRSTHDR(&msgh);
  cmsg->cmsg_level = SOL_SOCKET;
  cmsg->cmsg_type = SCM_RIGHTS;
  cmsg->cmsg_len = CMSG_LEN(sizeof(fds));
  memcpy(CMSG_DATA(cmsg), fds, sizeof(fds));
  msgh.msg_controllen = cmsg->cmsg_len;
  ssize_t r = sendmsg(hgc->sockfd, &msgh, 0);
  if (r < 0)
    abortmsgerrno("sendmsg failed");

  handleresponse(hgc);
  int32_t n;
  if (ctx->datasize != sizeof(n))
    abortmsg("unexpected size of attachio result");
  memcpy(&n, ctx->data, sizeof(n));
  n = ntohl(n);
  if (n != sizeof(fds) / sizeof(fds[0]))
    abortmsg("failed to send fds (n = %d)", n);
}

static void chdirtocwd(hgclient_t* hgc) {
  if (!getcwd(hgc->ctx.data, hgc->ctx.maxdatasize))
    abortmsgerrno("failed to getcwd");
  hgc->ctx.datasize = strlen(hgc->ctx.data);
  writeblockrequest(hgc, "chdir");
}

static void forwardumask(hgclient_t* hgc) {
  mode_t mask = umask(0);
  umask(mask);

  static const char command[] = "setumask\n";
  sendall(hgc->sockfd, command, sizeof(command) - 1);
  uint32_t data = htonl(mask);
  sendall(hgc->sockfd, &data, sizeof(data));
}

/*!
 * Open connection to per-user cmdserver
 *
 * If no background server running, returns NULL.
 */
hgclient_t* hgc_open(const char* sockname) {
  int fd = socket(AF_UNIX, SOCK_STREAM, 0);
  if (fd < 0)
    abortmsgerrno("cannot create socket");

  /* don't keep fd on fork(), so that it can be closed when the parent
   * process get terminated. */
  fsetcloexec(fd);

  struct sockaddr_un addr;
  addr.sun_family = AF_UNIX;

  /* use chdir to workaround small sizeof(sun_path) */
  int bakfd = -1;
  const char* basename = sockname;
  {
    const char* split = strrchr(sockname, '/');
    if (split && split != sockname) {
      if (split[1] == '\0')
        abortmsg("sockname cannot end with a slash");
      size_t len = split - sockname;
      char sockdir[len + 1];
      memcpy(sockdir, sockname, len);
      sockdir[len] = '\0';

      bakfd = open(".", O_DIRECTORY);
      if (bakfd == -1)
        abortmsgerrno("cannot open cwd");

      int r = chdir(sockdir);
      if (r != 0)
        abortmsgerrno("cannot chdir %s", sockdir);

      basename = split + 1;
    }
  }
  if (strlen(basename) >= sizeof(addr.sun_path))
    abortmsg("sockname is too long: %s", basename);
  strncpy(addr.sun_path, basename, sizeof(addr.sun_path));
  addr.sun_path[sizeof(addr.sun_path) - 1] = '\0';

  /* real connect */
  int r = connect(fd, (struct sockaddr*)&addr, sizeof(addr));
  if (r < 0) {
    if (errno != ENOENT && errno != ECONNREFUSED)
      abortmsgerrno("cannot connect to %s", sockname);
  }
  if (bakfd != -1) {
    fchdirx(bakfd);
    close(bakfd);
  }
  if (r < 0) {
    close(fd);
    return NULL;
  }
  debugmsg("connected to %s", addr.sun_path);

  hgclient_t* hgc = chg_mallocx(sizeof(hgclient_t));
  memset(hgc, 0, sizeof(*hgc));
  hgc->connectedat = chg_now();
  hgc->sockfd = fd;
  initcontext(&hgc->ctx);

  readhello(hgc);
  if (!(hgc->capflags & CAP_RUNCOMMAND))
    abortmsg("insufficient capability: runcommand");
  if (hgc->capflags & CAP_SETPROCNAME)
    updateprocname(hgc);
  if (hgc->capflags & CAP_ATTACHIO)
    attachio(hgc);
  if (hgc->capflags & CAP_CHDIR)
    chdirtocwd(hgc);
  if (hgc->capflags & CAP_SETUMASK)
    forwardumask(hgc);

  return hgc;
}

/*!
 * Close connection and free allocated memory
 */
void hgc_close(hgclient_t* hgc) {
  assert(hgc);
  freecontext(&hgc->ctx);
  close(hgc->sockfd);
  free(hgc);
}

pid_t hgc_peerpgid(const hgclient_t* hgc) {
  assert(hgc);
  return hgc->pgid;
}

pid_t hgc_peerpid(const hgclient_t* hgc) {
  assert(hgc);
  return hgc->pid;
}

unsigned long long hgc_versionhash(const hgclient_t* hgc) {
  assert(hgc);
  return hgc->versionhash;
}

/*!
 * Execute the specified Mercurial command
 *
 * @return result code
 */
int hgc_runcommand(hgclient_t* hgc, const char* const args[], size_t argsize) {
  assert(hgc);

  packcmdargs(&hgc->ctx, args, argsize);
  writeblockrequest(hgc, "runcommand");
  handleresponse(hgc);

  int32_t exitcode_n;
  if (hgc->ctx.datasize != sizeof(exitcode_n)) {
    abortmsg("unexpected size of exitcode");
  }
  memcpy(&exitcode_n, hgc->ctx.data, sizeof(exitcode_n));
  return ntohl(exitcode_n);
}

/*!
 * (Re-)send client's stdio channels so that the server can access to tty
 */
void hgc_attachio(hgclient_t* hgc) {
  assert(hgc);
  if (!(hgc->capflags & CAP_ATTACHIO))
    return;
  attachio(hgc);
}

/*!
 * Update server's environment variables
 *
 * @param envp  list of environment variables in "NAME=VALUE" format,
 *              terminated by NULL.
 */
void hgc_setenv(hgclient_t* hgc, const char* const envp[]) {
  assert(hgc && envp);
  if (!(hgc->capflags & CAP_SETENV))
    return;
  packcmdargs(&hgc->ctx, envp, /*argsize*/ -1);
  writeblockrequest(hgc, "setenv");
}

/*!
 * How many seconds have passed since connection.
 */
double hgc_elapsed(hgclient_t* hgc) {
  return chg_now() - hgc->connectedat;
}
