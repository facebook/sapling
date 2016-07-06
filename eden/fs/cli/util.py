#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import pwd
import time


class TimeoutError(Exception):
    pass


def poll_until(function, timeout, interval=0.2, timeout_ex=None):
    '''
    Call the specified function repeatedly until it returns non-None.
    Returns the function result.

    Sleep 'interval' seconds between calls.  If 'timeout' seconds passes
    before the function returns a non-None result, raise an exception.
    If a 'timeout_ex' argument is supplied, that exception object is
    raised, otherwise a TimeoutError is raised.
    '''
    end_time = time.time() + timeout
    while True:
        result = function()
        if result is not None:
            return result

        if time.time() >= end_time:
            if timeout_ex is not None:
                raise timeout_ex
            raise TimeoutError('timed out waiting on function {}'.format(
                function.__name__))

        time.sleep(interval)


def get_home_dir():
    home_dir = None
    if os.name == 'nt':
        home_dir = os.getenv('USERPROFILE')
    else:
        home_dir = os.getenv('HOME')
    if not home_dir:
        home_dir = pwd.getpwuid(os.getuid()).pw_dir
    return home_dir
