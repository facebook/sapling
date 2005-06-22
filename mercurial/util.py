# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os

def rename(src, dst):
    try:
        os.rename(src, dst)
    except:
        os.unlink(dst)
        os.rename(src, dst)

# Platfor specific varients
if os.name == 'nt':
    def pconvert(path):
        return path.replace("\\", "/")
else:
    def pconvert(path):
        return path

