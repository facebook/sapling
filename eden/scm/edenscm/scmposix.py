# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2013 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import array
import errno
import fcntl
import os


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
            arri = fcntl.ioctl(fd, TIOCGWINSZ, "\0" * 8)
            height, width = array.array(r"h", arri)[:2]
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
