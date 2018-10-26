/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

// 'systemd --user' refuses to start if the entire system is
// not managed by systemd. LD_PRELOAD this program to trick 'systemd --user'
// into thinking that the entire system is managed by systemd.

int sd_booted(void);

__attribute__((visibility("default"))) int sd_booted(void) {
  return 1;
}
