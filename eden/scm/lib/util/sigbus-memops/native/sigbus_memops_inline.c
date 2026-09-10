/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "sigbus_memops.h"
#include "sigbus_memops_config.h"

#if SIGBUS_MEMOPS_HAS_PROTECTION

// The inline assembly defines process-wide fault and recovery symbols, so the
// compiler must not make additional copies of these functions. Clang's
// optnone and GCC's noipa/noclone disable interprocedural cloning. Recovery
// leaves the asm normally so any compiler-generated epilogue is honored.
#if defined(__clang__)
#define SIGBUS_MEMOPS_NOINLINE __attribute__((noinline, optnone))
#elif __GNUC__ >= 9
#define SIGBUS_MEMOPS_NOINLINE __attribute__((noipa))
#else
#define SIGBUS_MEMOPS_NOINLINE __attribute__((noinline, noclone))
#endif

#if SIGBUS_MEMOPS_ARCH_X86_64

SIGBUS_MEMOPS_NOINLINE bool
sigbus_try_memcpy(void* dst, const void* src, size_t len) {
  int result;
  void* dst_cursor = dst;
  const void* src_cursor = src;
  size_t count = len;

  __asm__ volatile(
      ".globl sigbus_try_memcpy_fault_pc\n"
      ".hidden sigbus_try_memcpy_fault_pc\n"
      "sigbus_try_memcpy_fault_pc:\n"
      "rep movsb\n"
      "movl $1, %k[result]\n"
      "jmp 1f\n"
      ".globl sigbus_try_memcpy_recover_pc\n"
      ".hidden sigbus_try_memcpy_recover_pc\n"
      "sigbus_try_memcpy_recover_pc:\n"
      "xorl %k[result], %k[result]\n"
      "1:\n"
      : [result] "=&a"(result),
        [dst] "+D"(dst_cursor),
        [src] "+S"(src_cursor),
        [count] "+c"(count)
      :
      : "cc", "memory");
  return result;
}

SIGBUS_MEMOPS_NOINLINE bool sigbus_try_read(const void* src, size_t len) {
  int result;
  const void* src_cursor = src;
  size_t count = len;

  __asm__ volatile(
      ".globl sigbus_try_read_fault_pc\n"
      ".hidden sigbus_try_read_fault_pc\n"
      "sigbus_try_read_fault_pc:\n"
      "rep lodsb\n"
      "movl $1, %k[result]\n"
      "jmp 1f\n"
      ".globl sigbus_try_read_recover_pc\n"
      ".hidden sigbus_try_read_recover_pc\n"
      "sigbus_try_read_recover_pc:\n"
      "xorl %k[result], %k[result]\n"
      "1:\n"
      : [result] "=&a"(result), [src] "+S"(src_cursor), [count] "+c"(count)
      :
      : "cc", "memory");
  return result;
}

#endif
#endif
