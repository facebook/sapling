/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// 'systemd --user' refuses to start if the entire system is
// not managed by systemd. LD_PRELOAD this program to trick 'systemd --user'
// into thinking that the entire system is managed by systemd.

int sd_booted(void);

__attribute__((visibility("default"))) int sd_booted(void) {
  return 1;
}
