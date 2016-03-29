# fastmanifest.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
This extension adds fastmanifest, a treemanifest disk cache for speeding up
manifest comparison. It also contains utilities to investigate manifest access
patterns.


Configuration options:

[fastmanifest]
logfile = "" # Filename, is not empty will log access to any manifest
"""
from mercurial import extensions
from mercurial import manifest


class manifestaccesslogger(object):
    def __init__(self, logfile):
        self._logfile = logfile

    def revwrap(self, orig, *args, **kwargs):
        r = orig(*args, **kwargs)
        try:
            with open(self._logfile, "a") as f:
                f.write("%s\n" % r)
        except EnvironmentError as e:
            pass
        return r

def extsetup(ui):
    logfile = ui.config("fastmanifest", "logfile", "")
    if logfile:
        logger = manifestaccesslogger(logfile)
        extensions.wrapfunction(manifest.manifest, 'rev', logger.revwrap)
