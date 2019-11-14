/* whochanges - show processes accessing specified files
 *
 * Largely based on the example of fanotify (7) Linux manpage.
 *
 * Copyright (C) 2013, Heinrich Schuchardt <xypron.glpk@gmx.de>
 * Copyright (C) 2014, Michael Kerrisk <mtk.manpages@gmail.com>
 * Copyright (C) 2018, Facebook, Inc.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/fanotify.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

const char* timestamp() {
  struct timeval tv;
  static char buf[1024];
  size_t sz = sizeof(buf) - 1;
  ssize_t written = -1;

  if (gettimeofday(&tv, NULL) != 0) {
    perror("gettimeofday");
    exit(EXIT_FAILURE);
  }

  struct tm* time = localtime(&tv.tv_sec);

  if (time) {
    written = (ssize_t)strftime(buf, sz, "%Y-%m-%d %H:%M:%S", time);
    if ((written > 0) && ((size_t)written < sz)) {
      int w = snprintf(
          buf + written,
          sz - (size_t)written,
          ".%03d",
          (int)(tv.tv_usec / 1000));
      written = (w > 0) ? written + w : -1;
    }
    buf[written] = 0;
  }
  return buf;
}

/* Read all available fanotify events from the file descriptor 'fd' */

static void handle_events(int fd) {
  const struct fanotify_event_metadata* metadata;
  struct fanotify_event_metadata buf[200];
  ssize_t len;
  char path[PATH_MAX];
  ssize_t path_len;
  char procexe_path[PATH_MAX];
  char procfd_path[PATH_MAX];
  struct fanotify_response response;
  struct stat st;

  /* Loop while events can be read from fanotify file descriptor */

  for (;;) {
    /* Read some events */

    len = read(fd, (void*)&buf, sizeof(buf));
    if (len == -1 && errno != EAGAIN) {
      perror("read");
      exit(EXIT_FAILURE);
    }

    /* Check if end of available data reached */

    if (len <= 0)
      break;

    /* Point to the first event in the buffer */

    metadata = buf;

    /* Loop over all events in the buffer */

    while (FAN_EVENT_OK(metadata, len)) {
      /* Check that run-time and compile-time structures match */

      if (metadata->vers != FANOTIFY_METADATA_VERSION) {
        fprintf(stderr, "Mismatch of fanotify metadata version.\n");
        exit(EXIT_FAILURE);
      }

      /* metadata->fd contains either FAN_NOFD, indicating a
         queue overflow, or a file descriptor (a nonnegative
         integer). Here, we simply ignore queue overflow. */

      if (metadata->fd >= 0) {
        printf("[%s] pid %d ", timestamp(), metadata->pid);
        if (fstat(metadata->fd, &st) != 0) {
          perror("fstat");
          exit(EXIT_FAILURE);
        }

        snprintf(
            procexe_path,
            sizeof(procexe_path),
            "/proc/%d/exe",
            (int)metadata->pid);
        path_len = readlink(procexe_path, path, sizeof(path) - 1);
        if (path_len > 0) {
          path[path_len] = '\0';
          printf("(%s) ", path);
        }

        if (metadata->mask & FAN_OPEN) {
          printf("opens ");
        }

        if (metadata->mask & FAN_MODIFY) {
          printf("modifies ");
        }

        if (metadata->mask & FAN_ACCESS) {
          printf("reads ");
        }

        /* Handle closing of writable file event */

        if (metadata->mask & FAN_CLOSE_WRITE) {
          printf("closes ");
        }

        if (metadata->mask & FAN_CLOSE_NOWRITE) {
          printf("closes (no write) ");
        }

        /* Retrieve and print pathname of the accessed file */

        snprintf(
            procfd_path, sizeof(procfd_path), "/proc/self/fd/%d", metadata->fd);
        path_len = readlink(procfd_path, path, sizeof(path) - 1);
        if (path_len == -1) {
          perror("readlink");
          exit(EXIT_FAILURE);
        }

        path[path_len] = '\0';
        printf("%s (size %zu)\n", path, (size_t)st.st_size);
        fflush(stdout);

        /* Close the file descriptor of the event */

        close(metadata->fd);
      }

      /* Advance to next event */

      metadata = FAN_EVENT_NEXT(metadata, len);
    }
  }
}

int main(int argc, char* argv[]) {
  char buf;
  int fd, poll_num, i;
  nfds_t nfds;
  struct pollfd fds[2];

  /* Check mount point is supplied */

  if (argc != 2) {
    fprintf(stderr, "Usage: %s FILE [FILE...]\n", argv[0]);
    exit(EXIT_FAILURE);
  }

  setlinebuf(stdout);
  setlinebuf(stderr);

  fprintf(stderr, "Press enter key to terminate.\n");

  /* Create the file descriptor for accessing the fanotify API */

  fd = fanotify_init(
      FAN_CLOEXEC | FAN_CLASS_CONTENT | FAN_NONBLOCK, O_RDONLY | O_LARGEFILE);
  if (fd == -1) {
    perror("fanotify_init");
    fprintf(stderr, "(hint: try 'sudo'?)\n");
    exit(EXIT_FAILURE);
  }

  /* Mark the mount for:
     - permission events before opening files
     - notification events after closing a write-enabled
       file descriptor */

  for (i = 1; i < argc; ++i) {
    if (fanotify_mark(
            fd,
            FAN_MARK_ADD | FAN_MARK_DONT_FOLLOW,
            FAN_OPEN | FAN_MODIFY | FAN_ACCESS | FAN_CLOSE_WRITE |
                FAN_CLOSE_NOWRITE,
            AT_FDCWD,
            argv[i]) == -1) {
      perror("fanotify_mark");
      exit(EXIT_FAILURE);
    }
  }

  /* Prepare for polling */

  nfds = 2;

  /* Console input */

  fds[0].fd = STDIN_FILENO;
  fds[0].events = POLLIN;

  /* Fanotify input */

  fds[1].fd = fd;
  fds[1].events = POLLIN;

  /* This is the loop to wait for incoming events */

  fprintf(stderr, "Listening for events.\n");
  fprintf(stderr, "Note: file sizes are racy and can be inaccurate.\n");

  while (1) {
    poll_num = poll(fds, nfds, -1);
    if (poll_num == -1) {
      if (errno == EINTR) /* Interrupted by a signal */
        continue; /* Restart poll() */

      perror("poll"); /* Unexpected error */
      exit(EXIT_FAILURE);
    }

    if (poll_num > 0) {
      if (fds[0].revents & POLLIN) {
        /* Console input is available: empty stdin and quit */

        while (read(STDIN_FILENO, &buf, 1) > 0 && buf != '\n')
          continue;
        break;
      }

      if (fds[1].revents & POLLIN) {
        /* Fanotify events are available */

        handle_events(fd);
      }
    }
  }

  fprintf(stderr, "Listening for events stopped.\n");
  exit(EXIT_SUCCESS);
}
