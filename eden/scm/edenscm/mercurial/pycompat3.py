# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
This file contains helper code for pycompat that can only be successfully parsed by
Python 3.  This is in a separate file so that we can avoid importing it entirely
in Python 2.
"""

import abc


class ABC(metaclass=abc.ABCMeta):
    pass
