#!/usr/bin/env python
# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import, print_function

import errno
import os
import sys


for f in sys.argv[1:]:
    try:
        print(f, "->", os.readlink(f))
    except OSError as err:
        if err.errno != errno.EINVAL:
            raise
        print(f, "->", f, "not a symlink")

sys.exit(0)
