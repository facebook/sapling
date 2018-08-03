#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import collections
import configparser


# Convert the passed ConfigParser to a raw dictionary (without interpolation)
# Useful for updating configuration files in different formats.
def config_to_raw_dict(config: configparser.ConfigParser) -> collections.OrderedDict:
    rslt = collections.OrderedDict()  # type: collections.OrderedDict
    for section in config.sections():
        rslt[section] = collections.OrderedDict()
        for k, v in config.items(section, raw=True):
            rslt[section][k] = v
    return rslt
