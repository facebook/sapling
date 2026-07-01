/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Minimal FUSE daemon for testing FUSE_NOTIFY_INVAL_ENTRY on a negative dentry.
 *
 * Layout: /dir/file, initially absent. The "add" command makes it visible.
 *
 * stdin commands: add | reset | inval | sync | quit
 *
 * @noautodeps
 */

#define FUSE_USE_VERSION 34
#include <errno.h>
#include <fuse_lowlevel.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define ROOT_INO 1
#define DIR_INO 2
#define FILE_INO 3

#define TTL 2147483647.0

static struct fuse_session* se;
static atomic_int file_exists;

#define LOG(fmt, ...) fprintf(stderr, "\033[33m" fmt "\033[0m\n", ##__VA_ARGS__)

static struct stat make_stat(fuse_ino_t ino, int is_dir) {
  struct stat st = {.st_ino = ino, .st_uid = getuid(), .st_gid = getgid()};
  if (is_dir) {
    st.st_mode = S_IFDIR | 0755;
    st.st_nlink = 2;
  } else {
    st.st_mode = S_IFREG | 0644;
    st.st_nlink = 1;
    st.st_size = 4;
  }
  return st;
}

static void ll_init(void* ud, struct fuse_conn_info* c) {
  LOG("[init] proto=%u.%u", c->proto_major, c->proto_minor);
}

static void ll_lookup(fuse_req_t req, fuse_ino_t parent, const char* name) {
  int exists = atomic_load_explicit(&file_exists, memory_order_acquire);
  LOG("[LOOKUP] parent=%lu name=%s file_exists=%d", parent, name, exists);

  if (parent == ROOT_INO && !strcmp(name, "dir")) {
    struct fuse_entry_param e = {
        .ino = DIR_INO,
        .attr = make_stat(DIR_INO, 1),
        .attr_timeout = TTL,
        .entry_timeout = TTL};
    fuse_reply_entry(req, &e);
    return;
  }

  if (parent == DIR_INO && !strcmp(name, "file") && exists) {
    struct fuse_entry_param e = {
        .ino = FILE_INO,
        .attr = make_stat(FILE_INO, 0),
        .attr_timeout = TTL,
        .entry_timeout = TTL};
    fuse_reply_entry(req, &e);
    return;
  }

  struct fuse_entry_param e = {.ino = 0, .entry_timeout = TTL};
  fuse_reply_entry(req, &e);
}

static void
ll_getattr(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info* fi) {
  int exists = atomic_load_explicit(&file_exists, memory_order_acquire);
  LOG("[GETATTR] ino=%lu file_exists=%d", ino, exists);
  if (ino == ROOT_INO || ino == DIR_INO) {
    struct stat st = make_stat(ino, 1);
    fuse_reply_attr(req, &st, TTL);
  } else if (ino == FILE_INO && exists) {
    struct stat st = make_stat(ino, 0);
    fuse_reply_attr(req, &st, TTL);
  } else {
    fuse_reply_err(req, ENOENT);
  }
}

static void add_dirent(
    fuse_req_t req,
    char* buf,
    size_t buf_size,
    size_t* total,
    const char* name,
    fuse_ino_t ino,
    int is_dir,
    off_t next_off) {
  struct stat st = make_stat(ino, is_dir);
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
  int exists = atomic_load_explicit(&file_exists, memory_order_acquire);
  LOG("[READDIR] ino=%lu off=%ld file_exists=%d", ino, off, exists);
  if (ino != ROOT_INO && ino != DIR_INO) {
    fuse_reply_err(req, ENOTDIR);
    return;
  }

  char buf[8192];
  size_t total = 0;
  if (ino == ROOT_INO) {
    if (off <= 0) {
      add_dirent(req, buf, sizeof(buf), &total, ".", ROOT_INO, 1, 1);
    }
    if (off <= 1) {
      add_dirent(req, buf, sizeof(buf), &total, "..", ROOT_INO, 1, 2);
    }
    if (off <= 2) {
      add_dirent(req, buf, sizeof(buf), &total, "dir", DIR_INO, 1, 3);
    }
  } else {
    if (off <= 0) {
      add_dirent(req, buf, sizeof(buf), &total, ".", DIR_INO, 1, 1);
    }
    if (off <= 1) {
      add_dirent(req, buf, sizeof(buf), &total, "..", ROOT_INO, 1, 2);
    }
    if (off <= 2 && exists) {
      add_dirent(req, buf, sizeof(buf), &total, "file", FILE_INO, 0, 3);
    }
  }

  if (total > size) {
    total = size;
  }
  fuse_reply_buf(req, total ? buf : NULL, total);
}

static void ll_open(fuse_req_t req, fuse_ino_t ino, struct fuse_file_info* fi) {
  LOG("[OPEN] ino=%lu", ino);
  int exists = atomic_load_explicit(&file_exists, memory_order_acquire);
  if (ino != FILE_INO || !exists) {
    fuse_reply_err(req, ENOENT);
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
  const char contents[] = "data";
  int exists = atomic_load_explicit(&file_exists, memory_order_acquire);
  if (ino != FILE_INO || !exists) {
    fuse_reply_err(req, ENOENT);
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

static void invalidate_file(void) {
  int ret =
      fuse_lowlevel_notify_inval_entry(se, DIR_INO, "file", strlen("file"));
  LOG("[cmd] inval parent=%d name=file -> %s",
      DIR_INO,
      ret == 0 ? "ok" : strerror(-ret));
}

static void* cmd_thread(void* arg) {
  char line[256];
  while (fgets(line, sizeof(line), stdin)) {
    line[strcspn(line, "\n")] = 0;
    if (!*line) {
      continue;
    }
    if (!strcmp(line, "add")) {
      atomic_store_explicit(&file_exists, 1, memory_order_release);
      LOG("[cmd] file visible");
    } else if (!strcmp(line, "reset")) {
      atomic_store_explicit(&file_exists, 0, memory_order_release);
      LOG("[cmd] reset");
    } else if (!strcmp(line, "inval")) {
      invalidate_file();
    } else if (!strcmp(line, "sync")) {
      LOG("[cmd] sync");
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
