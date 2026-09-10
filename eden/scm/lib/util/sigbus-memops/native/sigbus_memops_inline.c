/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "sigbus_memops.h"
#include "sigbus_memops_config.h"

#include <stdint.h> // @manual

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

#elif SIGBUS_MEMOPS_ARCH_AARCH64

#if SIGBUS_MEMOPS_DARWIN_AARCH64
#define SIGBUS_MEMOPS_ASM_SYMBOL(name) "_" #name
#define SIGBUS_MEMOPS_ASM_VISIBILITY(name) \
  ".private_extern _" #name                \
  "\n"                                     \
  ".no_dead_strip _" #name "\n"
#else
#define SIGBUS_MEMOPS_ASM_SYMBOL(name) #name
#define SIGBUS_MEMOPS_ASM_VISIBILITY(name) ".hidden " #name "\n"
#endif

#define SIGBUS_MEMOPS_ASM_LABEL(name)                                         \
  ".globl " SIGBUS_MEMOPS_ASM_SYMBOL(name) "\n" SIGBUS_MEMOPS_ASM_VISIBILITY( \
      name) SIGBUS_MEMOPS_ASM_SYMBOL(name) ":\n"

SIGBUS_MEMOPS_NOINLINE bool
sigbus_try_memcpy(void* dst, const void* src, size_t len) {
  uintptr_t dst_and_result = (uintptr_t)dst;
  uintptr_t src_cursor = (uintptr_t)src;
  size_t count = len;

  __asm__ volatile(
      "cbz %[count], 2f\n"
      "1:\n"
      SIGBUS_MEMOPS_ASM_LABEL(sigbus_try_memcpy_fault_pc)
      "ldrb w3, [%[src]], 1\n"
      SIGBUS_MEMOPS_ASM_LABEL(sigbus_try_memcpy_store_fault_pc)
      "strb w3, [%[dst]], 1\n"
      "subs %[count], %[count], 1\n"
      "b.ne 1b\n"
      "2:\n"
      "mov %w[dst], 1\n"
      "b 3f\n"
      SIGBUS_MEMOPS_ASM_LABEL(sigbus_try_memcpy_recover_pc)
      "mov %w[dst], wzr\n"
      "3:\n"
      : [dst] "+&r"(dst_and_result),
        [src] "+&r"(src_cursor),
        [count] "+&r"(count)
      :
      : "x3", "cc", "memory");
  return (bool)dst_and_result;
}

SIGBUS_MEMOPS_NOINLINE bool sigbus_try_read(const void* src, size_t len) {
  uintptr_t src_and_result = (uintptr_t)src;
  size_t count = len;

  __asm__ volatile(
      "cbz %[count], 2f\n"
      "1:\n"
      SIGBUS_MEMOPS_ASM_LABEL(sigbus_try_read_fault_pc)
      "ldrb wzr, [%[src]], 1\n"
      "subs %[count], %[count], 1\n"
      "b.ne 1b\n"
      "2:\n"
      "mov %w[src], 1\n"
      "b 3f\n"
      SIGBUS_MEMOPS_ASM_LABEL(sigbus_try_read_recover_pc)
      "mov %w[src], wzr\n"
      "3:\n"
      : [src] "+&r"(src_and_result), [count] "+&r"(count)
      :
      : "cc", "memory");
  return (bool)src_and_result;
}

#endif
#endif
