# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals
import configparser


class EdenConfigInterpolator(configparser.Interpolation):
    ''' Python provides a couple of interpolation options but neither
        of them quite match the simplicity that we want.  This class
        will interpolate the keys of the provided map and replace
        those tokens with the values from the map.  There is no
        recursion or referencing of values from other sections of
        the config.
        Limiting the scope interpolation makes it easier to replicate
        this approach in the C++ implementation of the parser.
    '''

    def __init__(self, defaults):
        self._defaults = {}
        ''' pre-construct the token name that we're going to substitute.
            eg: {"foo": "bar"} is stored as {"${foo}": "bar"} internally
        '''
        for k, v in defaults.items():
            self._defaults['${' + k + '}'] = v

    def _interpolate(self, value):
        ''' simple brute force replacement using the defaults that were
            provided to us during construction '''
        for k, v in self._defaults.items():
            value = value.replace(k, v)
        return value

    def before_get(self, parser, section, option, value, defaults):
        return self._interpolate(value)

    def before_read(self, parser, section, option, value):
        return self._interpolate(value)
