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

    def makelock(info, pathname):
        ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
        os.write(ld, info)
        os.close(ld)

    def readlock(pathname):
        return file(pathname).read()
else:
    def pconvert(path):
        return path

    def makelock(info, pathname):
        os.symlink(info, pathname)

    def readlock(pathname):
        return os.readlink(pathname)


