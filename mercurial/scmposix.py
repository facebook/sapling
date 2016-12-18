from __future__ import absolute_import

import array
import errno
import fcntl
import os
import sys

from . import (
    encoding,
    osutil,
    pycompat,
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
    if pycompat.sysplatform == 'plan9':
        root = 'lib/mercurial'
    else:
        root = 'etc/mercurial'
    # old mod_python does not set sys.argv
    if len(getattr(sys, 'argv', [])) > 0:
        p = os.path.dirname(os.path.dirname(pycompat.sysargv[0]))
        if p != '/':
            path.extend(_rcfiles(os.path.join(p, root)))
    path.extend(_rcfiles('/' + root))
    return path

def userrcpath():
    if pycompat.sysplatform == 'plan9':
        return [encoding.environ['home'] + '/lib/hgrc']
    else:
        return [os.path.expanduser('~/.hgrc')]

def termsize(ui):
    try:
        import termios
        TIOCGWINSZ = termios.TIOCGWINSZ  # unavailable on IRIX (issue3449)
    except (AttributeError, ImportError):
        return 80, 24

    for dev in (ui.ferr, ui.fout, ui.fin):
        try:
            try:
                fd = dev.fileno()
            except AttributeError:
                continue
            if not os.isatty(fd):
                continue
            arri = fcntl.ioctl(fd, TIOCGWINSZ, '\0' * 8)
            height, width = array.array('h', arri)[:2]
            if width > 0 and height > 0:
                return width, height
        except ValueError:
            pass
        except IOError as e:
            if e[0] == errno.EINVAL:
                pass
            else:
                raise
    return 80, 24
