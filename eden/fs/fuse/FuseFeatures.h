/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef __linux__
#include "eden/fs/third-party/fuse_kernel_linux.h" // @manual
#endif

// Compile Eden's FUSE-over-io_uring transport when the FUSE protocol header
// provides the capability bit and the extended init flags used to negotiate it.
// Runtime negotiation still gates whether a mount uses it.
#if defined(FUSE_OVER_IO_URING) && defined(FUSE_INIT_EXT)
#define EDEN_HAVE_FUSE_IO_URING 1
#else
#define EDEN_HAVE_FUSE_IO_URING 0
#endif
