// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum.h: declarations for recalculating the checksums for intermediate
//             nodes in a tree.  this is for internal use only.
//
// no-check-code

#ifndef __FASTMANIFEST_CHECKSUM_H__
#define __FASTMANIFEST_CHECKSUM_H__

#include "tree.h"

void update_checksums(tree_t* tree);

#endif /* #ifndef __FASTMANIFEST_CHECKSUM_H__ */
