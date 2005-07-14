# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, errno

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

def copytree(src, dst, copyfile):
    """Copy a directory tree, files are copied using 'copyfile'."""
    names = os.listdir(src)
    os.mkdir(dst)

    for name in names:
        srcname = os.path.join(src, name)
        dstname = os.path.join(dst, name)
        if os.path.isdir(srcname):
            copytree(srcname, dstname, copyfile)
        elif os.path.isfile(srcname):
            copyfile(srcname, dstname)
        else:
            raise IOError("Not a regular file: %r" % srcname)

def _makelock_file(info, pathname):
    ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
    os.write(ld, info)
    os.close(ld)

def _readlock_file(pathname):
    return file(pathname).read()

# Platfor specific varients
if os.name == 'nt':
    nulldev = 'NUL:'

    def is_exec(f, last):
        return last

    def set_exec(f, mode):
        pass

    def pconvert(path):
        return path.replace("\\", "/")

    makelock = _makelock_file
    readlock = _readlock_file

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
        try:
            os.symlink(info, pathname)
        except OSError, why:
            if why.errno == errno.EEXIST:
                raise
            else:
                _makelock_file(info, pathname)

    def readlock(pathname):
        try:
            return os.readlink(pathname)
        except OSError, why:
            if why.errno == errno.EINVAL:
                return _readlock_file(pathname)
            else:
                raise
