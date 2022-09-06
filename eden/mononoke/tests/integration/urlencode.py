#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import sys
from urllib.parse import quote, unquote

if sys.argv[1] == "decode":
    print(unquote(sys.argv[2]))
elif sys.argv[1] == "encode":
    print(quote(sys.argv[2], safe=""))
else:
    print("argv[1] must be either 'decode' or 'encode'.", file=sys.stderr)
    exit(1)
