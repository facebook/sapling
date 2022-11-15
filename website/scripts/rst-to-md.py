#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import sys
from typing import Dict

"""
This script renders rst from Saplings raw command documentation into markdown.

This script takes a list of rst blobs serialized as a json via stdin, and
spits out a list of markdown blobs serialied as a json via stdout. The
list will keep the ordering of the blobs the same. i.e. out[i] is the
markdown of in[i].

This script should be run with `hg debugshell`. i.e.
```
$ echo '{"command1":"blob1","command2": "blob2"}' | hg debugshell rst-to-md.py
```
You can use a development version of sapling to run this command and you
will have access to the features and documentation included in that
development version. If you are building sapling via the `make local` method,
you should run this from the root of the sapling project folder:
```
$ echo '{"command1":"blob1","command2": "blob2"}' | ./hg debugshell rst-to-md.py
```
"""

raw_json = sys.stdin.readline()
rsts: Dict[str, str] = json.loads(raw_json)

mds: Dict[str, str] = {}

for (command, rst) in rsts.items():
    if not isinstance(rst, str):
        raise TypeError

    mds[command] = e.minirst.format(rst, style="md", keep=["verbose"])[0]  # noqa: F821


stdout = os.fdopen(1, "w")
json.dump(mds, stdout)
stdout.flush()
# intentionally leaving stdout open so debugshell could write to it if it must.
# this is better than cutting off an error message.
