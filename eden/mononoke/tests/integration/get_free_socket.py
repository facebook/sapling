#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# From https://unix.stackexchange.com/questions/55913/whats-the-easiest-way-to-find-an-unused-local-port

import socket


s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.bind(("", 0))
addr = s.getsockname()
print(addr[1])
s.close()
