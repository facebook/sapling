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
    def is_exec(f, last):
        return last

    def set_exec(f, mode):
        pass
    
    def pconvert(path):
        return path.replace("\\", "/")

    def makelock(info, pathname):
        ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
        os.write(ld, info)
        os.close(ld)

    def readlock(pathname):
        return file(pathname).read()
else:
    def is_exec(f, last):
        return (os.stat(f).st_mode & 0100 != 0)

    def set_exec(f, mode):
        s = os.stat(f).st_mode
        if (s & 0100 != 0) == mode:
            return
        if mode:
            # Turn on +x for every +r bit when making a file executable
            # and obey umask.
            umask = os.umask(0)
            os.umask(umask)
            os.chmod(f, s | (s & 0444) >> 2 & ~umask)
        else:
            os.chmod(f, s & 0666)

    def pconvert(path):
        return path

    def makelock(info, pathname):
        os.symlink(info, pathname)

    def readlock(pathname):
        return os.readlink(pathname)


