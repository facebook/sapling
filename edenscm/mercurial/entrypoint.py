#!/usr/bin/env python

# mercurial - scalable distributed SCM
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import os
import sys


def run():
    from edenscm import hgdemandimport
    from edenscm.mercurial import encoding

    if encoding.environ.get("HGUNICODEPEDANTRY", False):
        try:
            reload(sys)
            sys.setdefaultencoding("undefined")
        except NameError:
            pass

    if (
        sys.argv[1:5] == ["serve", "--cmdserver", "chgunix2", "--address"]
        and sys.argv[6:8] == ["--daemon-postexec", "chdir:/"]
        and "CHGINTERNALMARK" in encoding.environ
    ):
        # Shortcut path for chg server
        from edenscm.mercurial import dispatch

        dispatch.runchgserver()
    else:
        # Non-chg path
        try:
            if sys.version_info[0] < 3 or sys.version_info >= (3, 6):
                hgdemandimport.enable()
        except ImportError:
            sys.stderr.write(
                "abort: couldn't find mercurial libraries in [%s]\n"
                % " ".join(sys.path)
            )
            sys.stderr.write("(check your install and PYTHONPATH)\n")
            sys.exit(-1)
        from edenscm.mercurial import dispatch

        dispatch.run()


if __name__ == "__main__":
    run()
