# modulecheck.py - extension to check whether foreign extension are loaded
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import inspect
import os
import re
import sys

from mercurial import dispatch

dirname = os.path.dirname

# skip checking external modules
skipmodre = re.compile(r'\Amysql|remotenames|hgsubversion.*|lz4revlog\Z')

reporoot = dirname(dirname(__file__))
pyroot = dirname(os.__file__)
hgroot = dirname(dirname(dispatch.__file__))

def uisetup(ui):
    def _modulecheck():
        # whitelisted directories
        dirs = [reporoot, pyroot, hgroot]
        testtmp = os.environ.get('TESTTMP')
        if testtmp:
            dirs.append(testtmp)
        whitelistre = re.compile(r'\A(%s)/'
                                 % '|'.join(re.escape(d) for d in dirs))

        # blacklist hgext3rd in system path
        blacklistre = re.compile(r'\A%s/.*/hgext3rd/' % re.escape(pyroot))

        for name, mod in sys.modules.items():
            if skipmodre.match(name):
                continue
            try:
                path = inspect.getabsfile(mod)
            except Exception:
                continue
            if not path:
                continue
            if whitelistre.match(path) and not blacklistre.match(path):
                continue
            ui.write_err('external module %s imported: %s\n' % (name, path))

    ui.atexit(_modulecheck)
