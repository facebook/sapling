# debug.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import


class manifestaccesslogger(object):
    """Class to log manifest access and confirm our assumptions"""

    def __init__(self, ui):
        self._ui = ui

    def revwrap(self, orig, *args, **kwargs):
        """Wraps manifest.rev and log access"""
        r = orig(*args, **kwargs)
        logfile = self._ui.config("fastmanifest", "logfile")
        if logfile:
            try:
                with open(logfile, "a") as f:
                    f.write("%s\n" % r)
            except EnvironmentError:
                pass
        return r


class fixedcachelimit(object):
    """A fix cache limit expressed as a number of bytes"""

    def __init__(self, bytes):
        self._bytes = bytes

    def bytes(self):
        return self._bytes
