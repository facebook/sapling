/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Minimal FUSE daemon for testing bind mounts over FUSE directories.
 *
 * Layout:  /dir/fuse.txt
 *
 * stdin commands:
 *   same          future lookup("dir") returns DIR_INO
 *   different     future lookup("dir") returns ALT_DIR_INO
 *   inval_child   FUSE_NOTIFY_INVAL_INODE for DIR_INO
 *   inval_parent  FUSE_NOTIFY_INVAL_INODE for ROOT_INO
 *   inval_entry   FUSE_NOTIFY_INVAL_ENTRY for ROOT_INO/"dir"
 *   inc_epoch     FUSE_NOTIFY_INC_EPOCH
 *   quit
 *
 * @noautodeps
 */

#define FUSE_USE_VERSION 34
#include <errno.h>
#include <fuse_lowlevel.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define ROOT_INO 1
#define DIR_INO 2
#define ALT_DIR_INO 20
#define FUSE_FILE_INO 3

#define TTL 2147483647.0

static struct fuse_session* se;
static volatile int use_alt_dir_ino;
static unsigned int proto_minor;

#define LOG(fmt, ...) fprintf(stderr, "\033[33m" fmt "\033[0m\n", ##__VA_ARGS__)

static fuse_ino_t current_dir_ino(void) {
  return use_alt_dir_ino ? ALT_DIR_INO : DIR_INO;
}

static int is_dir_ino(fuse_ino_t ino) {
  return ino == DIR_INO || ino == ALT_DIR_INO;
}

static struct stat make_stat(fuse_ino_t ino, int is_dir) {
  struct stat st = {.st_ino = ino, .st_uid = getuid(), .st_gid = getgid()};
  if (is_dir) {
    st.st_mode = S_IFDIR | 0755;
    st.st_nlink = 2;
  } else {
    st.st_mode = S_IFREG | 0644;
    st.st_nlink = 1;
    st.st_size = 9;
  }
  return st;
}

static void ll_init(void* ud, struct fuse_conn_info* c) {
  proto_minor = c->proto_minor;
  LOG("[init] proto=%u.%u", c->proto_major, c->proto_minor);
}

static void ll_lookup(fuse_req_t req, fuse_ino_t parent, const char* name) {
  LOG("[LOOKUP] parent=%lu name=%s", parent, name);
  fuse_ino_t ino = 0;
  int is_dir = 0;
  if (parent == ROOT_INO && !strcmp(name, "dir")) {
    ino = current_dir_ino();
    is_dir = 1;
  } else if (is_dir_ino(parent) && !strcmp(name, "fuse.txt")) {
    ino = FUSE_FILE_INO;
  }

  if (!ino) {
    fuse_reply_err(req, ENOENT);
    return;
  }

  struct fuse_entry_param e = {
      .ino = ino,
      .attr = make_stat(ino, is_dir),
      .attr_timeout = TTL,
      .entry_timeout = TTL};
  fuse_reply_entry(req, &e);
}

static void
ll_getattr(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info* fi) {
  LOG("[GETATTR] ino=%lu", ino);
  if (ino != ROOT_INO && !is_dir_ino(ino) && ino != FUSE_FILE_INO) {
    fuse_reply_err(req, ENOENT);
    return;
  }
  struct stat st = make_stat(ino, ino == ROOT_INO || is_dir_ino(ino));
  fuse_reply_attr(req, &st, TTL);
}

static void add_dirent(
    fuse_req_t req,
    char* buf,
    size_t buf_size,
    size_t* total,
    const char* name,
    fuse_ino_t ino,
    off_t next_off) {
  struct stat st = make_stat(ino, ino == ROOT_INO || is_dir_ino(ino));
  size_t entry_size = fuse_add_direntry(
      req, buf + *total, buf_size - *total, name, &st, next_off);
  *total += entry_size;
}

static void ll_readdir(
    fuse_req_t req,
    fuse_ino_t ino,
    size_t size,
    off_t off,
    struct fuse_file_info* fi) {
  LOG("[READDIR] ino=%lu off=%ld", ino, off);
  if (ino != ROOT_INO && !is_dir_ino(ino)) {
    fuse_reply_err(req, ENOTDIR);
    return;
  }

  char buf[8192];
  size_t total = 0;
  if (ino == ROOT_INO) {
    if (off <= 0) {
      add_dirent(req, buf, sizeof(buf), &total, ".", ROOT_INO, 1);
    }
    if (off <= 1) {
      add_dirent(req, buf, sizeof(buf), &total, "..", ROOT_INO, 2);
    }
    if (off <= 2) {
      add_dirent(req, buf, sizeof(buf), &total, "dir", current_dir_ino(), 3);
    }
  } else {
    if (off <= 0) {
      add_dirent(req, buf, sizeof(buf), &total, ".", ino, 1);
    }
    if (off <= 1) {
      add_dirent(req, buf, sizeof(buf), &total, "..", ROOT_INO, 2);
    }
    if (off <= 2) {
      add_dirent(req, buf, sizeof(buf), &total, "fuse.txt", FUSE_FILE_INO, 3);
    }
  }

  if (total > size) {
    total = size;
  }
  fuse_reply_buf(req, total ? buf : NULL, total);
}

static void ll_open(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info* fi) {
  LOG("[OPEN] ino=%lu", ino);
  if (ino != FUSE_FILE_INO) {
    fuse_reply_err(req, EISDIR);
    return;
  }
  fuse_reply_open(req, fi);
}

static void ll_read(
    fuse_req_t req,
    fuse_ino_t ino,
    size_t size,
    off_t off,
    struct fuse_file_info* fi) {
  LOG("[READ] ino=%lu off=%ld", ino, off);
  const char contents[] = "fuse file";
  if (ino != FUSE_FILE_INO) {
    fuse_reply_err(req, EISDIR);
    return;
  }
  if (off >= (off_t)sizeof(contents) - 1) {
    fuse_reply_buf(req, NULL, 0);
    return;
  }
  size_t available = sizeof(contents) - 1 - off;
  fuse_reply_buf(req, contents + off, size < available ? size : available);
}

static void ll_forget(fuse_req_t req, fuse_ino_t ino, uint64_t nl) {
  LOG("[FORGET] ino=%lu nlookup=%lu", ino, nl);
  fuse_reply_none(req);
}

static void invalidate_inode(fuse_ino_t ino, const char* label) {
  int ret = fuse_lowlevel_notify_inval_inode(se, ino, 0, 0);
  LOG("[cmd] %s ino=%lu -> %s", label, ino, ret == 0 ? "ok" : strerror(-ret));
}

static void invalidate_entry(void) {
  int ret =
      fuse_lowlevel_notify_inval_entry(se, ROOT_INO, "dir", strlen("dir"));
  LOG("[cmd] inval_entry parent=%d name=dir -> %s",
      ROOT_INO,
      ret == 0 ? "ok" : strerror(-ret));
}

static void inc_epoch(void) {
  if (proto_minor < 44) {
    LOG("[cmd] inc_epoch -> ENOSYS (proto_minor=%u < 44)", proto_minor);
    return;
  }
  struct {
    uint32_t len;
    int32_t code;
    uint64_t unique;
  } msg = {sizeof(msg), 8 /* FUSE_NOTIFY_INC_EPOCH */, 0};
  ssize_t n = write(fuse_session_fd(se), &msg, sizeof(msg));
  LOG("[cmd] inc_epoch -> %s",
      n == (ssize_t)sizeof(msg) ? "ok" : strerror(errno));
}

static void* cmd_thread(void* arg) {
  char line[256];
  while (fgets(line, sizeof(line), stdin)) {
    line[strcspn(line, "\n")] = 0;
    if (!*line) {
      continue;
    }
    if (!strcmp(line, "same")) {
      use_alt_dir_ino = 0;
      LOG("[cmd] future dir inode=%d", DIR_INO);
    } else if (!strcmp(line, "different")) {
      use_alt_dir_ino = 1;
      LOG("[cmd] future dir inode=%d", ALT_DIR_INO);
    } else if (!strcmp(line, "inval_child")) {
      invalidate_inode(DIR_INO, "inval_child");
    } else if (!strcmp(line, "inval_parent")) {
      invalidate_inode(ROOT_INO, "inval_parent");
    } else if (!strcmp(line, "inval_entry")) {
      invalidate_entry();
    } else if (!strcmp(line, "inc_epoch")) {
      inc_epoch();
    } else if (!strcmp(line, "quit")) {
      fuse_session_exit(se);
      break;
    } else {
      LOG("[cmd] unknown: %s", line);
    }
  }
  return NULL;
}

static const struct fuse_lowlevel_ops ops = {
    .init = ll_init,
    .lookup = ll_lookup,
    .getattr = ll_getattr,
    .readdir = ll_readdir,
    .open = ll_open,
    .read = ll_read,
    .forget = ll_forget,
};

int main(int argc, char* argv[]) {
  struct fuse_args args = FUSE_ARGS_INIT(argc, argv);
  struct fuse_cmdline_opts opts = {0};
  if (fuse_parse_cmdline(&args, &opts) || !opts.mountpoint) {
    fprintf(stderr, "Usage: %s <mountpoint>\n", argv[0]);
    return 1;
  }

  se = fuse_session_new(&args, &ops, sizeof(ops), NULL);
  if (!se || fuse_set_signal_handlers(se) ||
      fuse_session_mount(se, opts.mountpoint)) {
    if (se) {
      fuse_session_destroy(se);
    }
    free(opts.mountpoint);
    return 1;
  }

  pthread_t tid;
  pthread_create(&tid, NULL, cmd_thread, NULL);
  int ret = fuse_session_loop(se);
  fuse_session_unmount(se);
  fuse_remove_signal_handlers(se);
  fuse_session_destroy(se);
  free(opts.mountpoint);
  return ret;
}
