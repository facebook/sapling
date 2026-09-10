/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#ifndef SIGBUS_MEMOPS_CONFIG_H
#define SIGBUS_MEMOPS_CONFIG_H

#if !defined(SIGBUS_MEMOPS_FORCE_FALLBACK) && \
    (defined(__GNUC__) || defined(__clang__))
#if defined(__x86_64__)
#if defined(__linux__)
#define SIGBUS_MEMOPS_ARCH_X86_64 1
#endif
#endif
#endif

#ifndef SIGBUS_MEMOPS_ARCH_X86_64
#define SIGBUS_MEMOPS_ARCH_X86_64 0
#endif

#ifndef SIGBUS_MEMOPS_ARCH_AARCH64
#define SIGBUS_MEMOPS_ARCH_AARCH64 0
#endif

#if SIGBUS_MEMOPS_ARCH_AARCH64 && defined(__APPLE__) && defined(__MACH__)
#define SIGBUS_MEMOPS_DARWIN_AARCH64 1
#else
#define SIGBUS_MEMOPS_DARWIN_AARCH64 0
#endif

#if SIGBUS_MEMOPS_ARCH_X86_64 || SIGBUS_MEMOPS_ARCH_AARCH64
#define SIGBUS_MEMOPS_HAS_PROTECTION 1
#else
#define SIGBUS_MEMOPS_HAS_PROTECTION 0
#endif

#endif
