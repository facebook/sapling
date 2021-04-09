// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// sha1.h - wrapper functions around the underlying SHA-1 implementation.
//
// no-check-code
#ifndef FBHGEXT_CLIB_SHA1_H
#define FBHGEXT_CLIB_SHA1_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef SHA1_USE_SHA1DC
#include <lib/third-party/sha1dc/sha1.h>
#include <stdlib.h>

typedef SHA1_CTX fbhg_sha1_ctx_t;

static inline int fbhg_sha1_init(fbhg_sha1_ctx_t* ctx) {
  SHA1DCInit(ctx);
  SHA1DCSetSafeHash(ctx, 0);
  SHA1DCSetUseDetectColl(ctx, 0);
  return 0;
}

static inline int
fbhg_sha1_update(fbhg_sha1_ctx_t* ctx, const void* data, unsigned long length) {
  SHA1DCUpdate(ctx, (const char*)data, length);
  return 0;
}

static inline int fbhg_sha1_final(unsigned char* md, fbhg_sha1_ctx_t* ctx) {
  return SHA1DCFinal(md, ctx);
}
#else
#include <openssl/sha.h>

typedef SHA_CTX fbhg_sha1_ctx_t;

static inline int fbhg_sha1_init(fbhg_sha1_ctx_t* ctx) {
  return SHA1_Init(ctx);
}

static inline int
fbhg_sha1_update(fbhg_sha1_ctx_t* ctx, const void* data, unsigned long length) {
  return SHA1_Update(ctx, data, length);
}

static inline int fbhg_sha1_final(unsigned char* md, fbhg_sha1_ctx_t* ctx) {
  return SHA1_Final(md, ctx);
}
#endif

#ifdef __cplusplus
} /* extern C */
#endif

#endif /* FBHGEXT_CLIB_SHA1_H */
