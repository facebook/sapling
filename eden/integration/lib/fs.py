#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import json
import subprocess
import typing

from .find_executables import FindExe


def getxattr(abspath: str, attr: str) -> str:
    raw_stdout = subprocess.check_output(
        [FindExe.FSATTR, "--attrName", attr, "--fileName", abspath]
    )
    result = json.loads(raw_stdout)
    # fsattr should always return a string here.  We just cast the result,
    # without actually validating it for now.
    return typing.cast(str, result)


def listxattr(abspath: str) -> typing.Dict[str, str]:
    raw_stdout = subprocess.check_output([FindExe.FSATTR, "--fileName", abspath])
    result = json.loads(raw_stdout)
    # fsattr should always return a dictionary here.  We just cast the
    # result, without actually validating it for now.
    return typing.cast(typing.Dict[str, str], result)
