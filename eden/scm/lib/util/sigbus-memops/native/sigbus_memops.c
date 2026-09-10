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
#include "sigbus_memops_config.h"

#include <string.h>

bool sigbus_is_protected(void) {
  return SIGBUS_MEMOPS_HAS_PROTECTION;
}

#if SIGBUS_MEMOPS_WINDOWS_SEH

// @lint-ignore SPELL
#include <excpt.h>
#include <windows.h>

static int sigbus_memops_filter(unsigned long code) {
  return code == EXCEPTION_IN_PAGE_ERROR ? EXCEPTION_EXECUTE_HANDLER
                                         : EXCEPTION_CONTINUE_SEARCH;
}

bool sigbus_test_raise_in_page_error(void) {
  __try {
    RaiseException(EXCEPTION_IN_PAGE_ERROR, 0, 0, NULL);
  } __except (sigbus_memops_filter(GetExceptionCode())) {
    return true;
  }
  return false;
}

bool sigbus_try_memcpy(void* dst, const void* src, size_t len) {
  __try {
    if (len != 0) {
      memcpy(dst, src, len);
    }
    return true;
  } __except (sigbus_memops_filter(GetExceptionCode())) {
    return false;
  }
}

bool sigbus_try_read(const void* src, size_t len) {
  __try {
    const volatile unsigned char* bytes = src;
    for (size_t i = 0; i < len; ++i) {
      (void)bytes[i];
    }
    return true;
  } __except (sigbus_memops_filter(GetExceptionCode())) {
    return false;
  }
}

#elif SIGBUS_MEMOPS_HAS_PROTECTION

#include <signal.h>
#include <stdint.h>

#if SIGBUS_MEMOPS_DARWIN_AARCH64
#include <mach/arm/thread_status.h>
#include <sys/ucontext.h>
#else
#include <ucontext.h>
#endif

// Defined by labels in the protected functions' inline assembly.
extern void sigbus_try_memcpy_fault_pc(void);
extern void sigbus_try_memcpy_recover_pc(void);
extern void sigbus_try_read_fault_pc(void);
extern void sigbus_try_read_recover_pc(void);
#if SIGBUS_MEMOPS_ARCH_AARCH64
// Only AArch64 splits the destination write into its own protected
// instruction; x86-64 faults on the same `rep movsb` for both accesses.
extern void sigbus_try_memcpy_store_fault_pc(void);
#endif

bool sigbus_try_handle(int signo, siginfo_t* info, void* opaque) {
  if (signo != SIGBUS || info == NULL || opaque == NULL) {
    return false;
  }
#ifdef BUS_MCEERR_AO
  if (info->si_code == BUS_MCEERR_AO) {
    return false;
  }
#endif

  ucontext_t* context = opaque;

#if SIGBUS_MEMOPS_ARCH_X86_64
  greg_t* registers = context->uc_mcontext.gregs;

  // x86-64 reports source-read and destination-write faults at the same
  // `rep movsb` program counter.
  uintptr_t pc = (uintptr_t)registers[REG_RIP];
  if (pc == (uintptr_t)&sigbus_try_memcpy_fault_pc) {
    registers[REG_RIP] = (greg_t)(uintptr_t)&sigbus_try_memcpy_recover_pc;
    return true;
  }

  if (pc == (uintptr_t)&sigbus_try_read_fault_pc) {
    registers[REG_RIP] = (greg_t)(uintptr_t)&sigbus_try_read_recover_pc;
    return true;
  }
#elif SIGBUS_MEMOPS_ARCH_AARCH64
#if SIGBUS_MEMOPS_DARWIN_AARCH64
  arm_thread_state64_t* registers = &context->uc_mcontext->__ss;
  void (*pc)(void) = arm_thread_state64_get_pc_fptr(*registers);
  if (pc == &sigbus_try_memcpy_fault_pc ||
      pc == &sigbus_try_memcpy_store_fault_pc) {
    arm_thread_state64_set_pc_fptr(*registers, &sigbus_try_memcpy_recover_pc);
    return true;
  }

  if (pc == &sigbus_try_read_fault_pc) {
    arm_thread_state64_set_pc_fptr(*registers, &sigbus_try_read_recover_pc);
    return true;
  }
#else
  uintptr_t pc = (uintptr_t)context->uc_mcontext.pc;
  if (pc == (uintptr_t)&sigbus_try_memcpy_fault_pc ||
      pc == (uintptr_t)&sigbus_try_memcpy_store_fault_pc) {
    context->uc_mcontext.pc = (uintptr_t)&sigbus_try_memcpy_recover_pc;
    return true;
  }

  if (pc == (uintptr_t)&sigbus_try_read_fault_pc) {
    context->uc_mcontext.pc = (uintptr_t)&sigbus_try_read_recover_pc;
    return true;
  }
#endif
#endif

  return false;
}

#else

#ifndef _WIN32
bool sigbus_try_handle(int signo, siginfo_t* info, void* opaque) {
  (void)signo;
  (void)info;
  (void)opaque;
  return false;
}
#endif

bool sigbus_try_memcpy(void* dst, const void* src, size_t len) {
  if (len == 0) {
    return true;
  }
  memcpy(dst, src, len);
  return true;
}

bool sigbus_try_read(const void* src, size_t len) {
  if (len == 0) {
    return true;
  }
  const volatile unsigned char* bytes = src;
  for (size_t i = 0; i < len; ++i) {
    (void)bytes[i];
  }
  return true;
}

#endif
