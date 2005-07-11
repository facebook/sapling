# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os

def unique(g):
    seen = {}
    for f in g:
        if f not in seen:
            seen[f] = 1
            yield f

class CommandError(Exception): pass

def explain_exit(code):
    """return a 2-tuple (desc, code) describing a process's status"""
    if os.WIFEXITED(code):
        val = os.WEXITSTATUS(code)
        return "exited with status %d" % val, val
    elif os.WIFSIGNALED(code):
        val = os.WTERMSIG(code)
        return "killed by signal %d" % val, val
    elif os.WIFSTOPPED(code):
        val = os.WSTOPSIG(code)
        return "stopped by signal %d" % val, val
    raise ValueError("invalid exit code")

def system(cmd, errprefix=None):
    """execute a shell command that must succeed"""
    rc = os.system(cmd)
    if rc:
        errmsg = "%s %s" % (os.path.basename(cmd.split(None, 1)[0]),
                            explain_exit(rc)[0])
        if errprefix:
            errmsg = "%s: %s" % (errprefix, errmsg)
        raise CommandError(errmsg)

def rename(src, dst):
    try:
        os.rename(src, dst)
    except:
        os.unlink(dst)
        os.rename(src, dst)

# Platfor specific varients
if os.name == 'nt':
    nulldev = 'NUL:'

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
    nulldev = '/dev/null'

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
