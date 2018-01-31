#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import json
import os
import subprocess

from .find_executables import FSATTR

def getxattr(abspath, attr):
    raw_stdout = subprocess.check_output([FSATTR, '--attrName', attr, '--fileName', abspath])
    return json.loads(raw_stdout)


def listxattr(abspath):
    raw_stdout = subprocess.check_output([FSATTR, '--fileName', abspath])
    return json.loads(raw_stdout)
