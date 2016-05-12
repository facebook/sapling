# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import os
import subprocess

def getxattr(abspath, attr):
    if is_linux():
        stdout = subprocess.check_output(['getfattr', '-n', attr, abspath])
        lines = stdout.split('\n')
        if len(lines) < 1:
            return None
        line = lines[1]
        index = line.index('=')
        # Value is in double-quotes, so must modify index appropriately.
        if line[:index] == attr:
            return line[index + 2:-1]
        else:
            raise Exception('xattr %s not found' % attr)
    else:
        raise Exception('Only supported on Linux')


def listxattr(abspath):
    if is_linux():
        stdout = subprocess.check_output(['getfattr', '-d', abspath])
        lines = stdout.split('\n')
        if not lines:
            return {}
        attrs = {}
        for line in lines[1:]:
            if not line:
                continue
            index = line.index('=')
            # Value is in double-quotes, so must modify index appropriately.
            attrs[line[:index]] = line[index + 2:-1]
        return attrs
    else:
        raise Exception('Only supported on Linux')


def is_linux():
    return os.uname()[0] == 'Linux'
