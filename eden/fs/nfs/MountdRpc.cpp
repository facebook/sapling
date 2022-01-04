/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/MountdRpc.h"

namespace facebook::eden {

EDEN_XDR_SERDE_IMPL(mountres3_ok, fhandle3, auth_flavors);

}

#endif
