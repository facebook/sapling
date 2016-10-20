from __future__ import absolute_import

import errno
import fcntl
import os
import sys

from . import (
    encoding,
    osutil,
)

def _rcfiles(path):
    rcs = [os.path.join(path, 'hgrc')]
    rcdir = os.path.join(path, 'hgrc.d')
    try:
        rcs.extend([os.path.join(rcdir, f)
                    for f, kind in osutil.listdir(rcdir)
                    if f.endswith(".rc")])
    except OSError:
        pass
    return rcs

def systemrcpath():
    path = []
    if sys.platform == 'plan9':
        root = 'lib/mercurial'
    else:
        root = 'etc/mercurial'
    # old mod_python does not set sys.argv
    if len(getattr(sys, 'argv', [])) > 0:
        p = os.path.dirname(os.path.dirname(sys.argv[0]))
        if p != '/':
            path.extend(_rcfiles(os.path.join(p, root)))
    path.extend(_rcfiles('/' + root))
    return path

def userrcpath():
    if sys.platform == 'plan9':
        return [encoding.environ['home'] + '/lib/hgrc']
    else:
        return [os.path.expanduser('~/.hgrc')]

def termwidth():
    try:
        import array
        import termios
        for dev in (sys.stderr, sys.stdout, sys.stdin):
            try:
                try:
                    fd = dev.fileno()
                except AttributeError:
                    continue
                if not os.isatty(fd):
                    continue
                try:
                    arri = fcntl.ioctl(fd, termios.TIOCGWINSZ, '\0' * 8)
                    width = array.array('h', arri)[1]
                    if width > 0:
                        return width
                except AttributeError:
                    pass
            except ValueError:
                pass
            except IOError as e:
                if e[0] == errno.EINVAL:
                    pass
                else:
                    raise
    except ImportError:
        pass
    return 80
