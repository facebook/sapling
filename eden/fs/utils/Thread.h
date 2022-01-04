/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook {
namespace eden {

/**
 * Disable pthread cancellation for the calling thread. This improves
 * performance in glibc for cancellation point syscalls by avoiding two atomic
 * CAS operations per syscall. See pthreads(7) for the list of functions that
 * are defined to be cancellation points.
 */
void disablePthreadCancellation();

} // namespace eden
} // namespace facebook
