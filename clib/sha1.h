// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// sha1.h - wrapper functions around the underlying SHA-1 implementation.
//
// no-check-code
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#include <stdlib.h>
#include <sha1dc/sha1.h>

typedef SHA1_CTX fbhg_sha1_ctx_t;

static inline int fbhg_sha1_init(fbhg_sha1_ctx_t* ctx) {
  SHA1DCInit(ctx);
  return 0;
}

static inline int
fbhg_sha1_update(fbhg_sha1_ctx_t* ctx, const void* data, unsigned long length) {
  SHA1DCUpdate(ctx, (const unsigned char*)data, length);
  return 0;
}

static inline int fbhg_sha1_final(unsigned char* md, fbhg_sha1_ctx_t* ctx) {
  return SHA1DCFinal(md, ctx);
}

#ifdef __cplusplus
} /* extern C */
#endif
