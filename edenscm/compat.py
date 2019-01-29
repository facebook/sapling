# Copyright Facebook, Inc. 2018
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Compatibility layer for code relying on mercurial and hgext being the
top-level modules.

The main users are code outside the main code base such as merge drivers and
hook drivers.
"""


import sys


sys.path[0:0] = sys.modules["edenscm"].__path__
