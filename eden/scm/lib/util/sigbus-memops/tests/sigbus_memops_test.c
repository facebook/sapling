/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "sigbus_memops.h"

#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef _WIN32
#include <sys/mman.h>
#if defined(__APPLE__) && defined(__MACH__)
#include <sys/ucontext.h>
#else
#include <ucontext.h>
#endif
#include <unistd.h>
#endif

#define CHECK(expression)                                 \
  do {                                                    \
    if (!(expression)) {                                  \
      fprintf(stderr, "check failed: %s\n", #expression); \
      exit(1);                                            \
    }                                                     \
  } while (0)

#ifndef _WIN32
struct signal_state {
  volatile sig_atomic_t expect_unhandled;
  volatile sig_atomic_t saw_unhandled;
  volatile sig_atomic_t last_handled_code;
};

// The signal handler and test body can only communicate through global state.
static struct signal_state
    signal_state; // NOLINT(facebook-avoid-non-const-global-variables)

static void on_sigbus(int signo, siginfo_t* info, void* context) {
  if (info != NULL && sigbus_try_handle(signo, info, context)) {
    signal_state.last_handled_code = info->si_code;
    return;
  }

  if (signal_state.expect_unhandled) {
    signal_state.saw_unhandled = 1;
    return;
  }

  _exit(128 + signo);
}
#endif

int main(void) {
  if (!sigbus_is_protected()) {
    return 0;
  }

#ifndef _WIN32

  long page_size_long = sysconf(_SC_PAGESIZE);
  CHECK(page_size_long > 0);
  size_t page_size = (size_t)page_size_long;

  char path[] = "/tmp/sigbus-memops.XXXXXX";
  int fd = mkstemp(path);
  CHECK(fd >= 0);
  CHECK(unlink(path) == 0);
  CHECK(ftruncate(fd, (off_t)page_size) == 0);

  uint8_t* mapping =
      mmap(NULL, 2 * page_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if (mapping == MAP_FAILED || mapping == NULL) {
    return 1;
  }

  struct sigaction action = {0};
  struct sigaction old_action;
  action.sa_sigaction = on_sigbus;
  action.sa_flags = SA_SIGINFO;
  CHECK(sigemptyset(&action.sa_mask) == 0);
  CHECK(sigaction(SIGBUS, &action, &old_action) == 0);

#ifdef BUS_MCEERR_AO
  siginfo_t asynchronous_mce = {.si_code = BUS_MCEERR_AO};
  ucontext_t synthetic_context = {0};
  CHECK(!sigbus_try_handle(SIGBUS, &asynchronous_mce, &synthetic_context));
#endif

  signal_state.expect_unhandled = 1;
  CHECK(raise(SIGBUS) == 0);
  CHECK(signal_state.saw_unhandled == 1);
  signal_state.expect_unhandled = 0;

  uint8_t source[32];
  for (size_t i = 0; i < sizeof(source); ++i) {
    source[i] = (uint8_t)(0x80 + i);
  }

  CHECK(sigbus_try_memcpy(mapping, source, sizeof(source)));
  CHECK(memcmp(mapping, source, sizeof(source)) == 0);
  CHECK(sigbus_try_read(mapping, sizeof(source)));
  CHECK(sigbus_try_read(NULL, 0));

  uint8_t* crossing = mapping + page_size - 8;
  CHECK(!sigbus_try_memcpy(crossing, source, 16));
#if defined(__APPLE__) && defined(__MACH__)
  CHECK(signal_state.last_handled_code == BUS_ADRALN);
#else
  CHECK(signal_state.last_handled_code == BUS_ADRERR);
#endif
  CHECK(memcmp(crossing, source, 8) == 0);

  uint8_t source_fault_destination = 0;
  CHECK(!sigbus_try_memcpy(&source_fault_destination, mapping + page_size, 1));
  CHECK(!sigbus_try_read(mapping + page_size, 1));
  CHECK(!sigbus_try_read(mapping + page_size - 8, 16));

  CHECK(sigaction(SIGBUS, &old_action, NULL) == 0);
  CHECK(munmap(mapping, 2 * page_size) == 0);
  CHECK(close(fd) == 0);
  return 0;
#else
  return 1;
#endif
}
