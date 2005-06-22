# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os

if os.name == 'nt':
    def pconvert(path):
        return path.replace("\\", "/")
else:
    def pconvert(path):
        return path

