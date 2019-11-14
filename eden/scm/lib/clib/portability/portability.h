// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef FBHGEXT_CLIB_PORTABILITY_PORTABILITY_H
#define FBHGEXT_CLIB_PORTABILITY_PORTABILITY_H

#if defined(_MSC_VER)
/* MSVC2015 supports compound literals in C mode (/TC)
   but does not support them in C++ mode (/TP) */
#if defined(__cplusplus)
#define COMPOUND_LITERAL(typename_) typename_
#else /* #if defined(__cplusplus) */
#define COMPOUND_LITERAL(typename_) (typename_)
#endif /* #if defined(__cplusplus) */
#else /* #if defined(_MSC_VER) */
#define COMPOUND_LITERAL(typename_) (typename_)
#endif /* #if defined(_MSC_VER) */

#if defined(_MSC_VER)
#define PACKEDSTRUCT(__Declaration__) \
  __pragma(pack(push, 1)) __Declaration__ __pragma(pack(pop))
#else
#define PACKEDSTRUCT(__Declaration__) __Declaration__ __attribute__((packed))
#endif

#endif /* #ifndef FBHGEXT_CLIB_PORTABILITY_PORTABILITY_H */
