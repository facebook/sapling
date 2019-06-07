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


def run(binaryexecution):
    # entrypoint is in mercurial/ dir, while we want 'from mercurial import ...',
    # 'from hgext import ...' and 'from hgdemandimport import ...' to work
    # so we are adding their parent directory to be the first item of sys.path
    # Do not follow symlinks (ex. do not use "realpath"). It breaks buck build.
    filedir = os.path.dirname(os.path.abspath(__file__))
    libdir = os.path.dirname(os.path.dirname(filedir))
    if sys.path[0] != libdir:
        sys.path.insert(0, libdir)

    for element in list(sys.path):
        if os.path.realpath(filedir) == os.path.realpath(element):
            # the directory of entrypoint.py is mercurial/
            # and it should not be present in sys.path, as we use absolute_import
            sys.path.remove(element)

    from edenscm import hgdemandimport

    hgdemandimport.tryenableembedded()

    from edenscm.mercurial import encoding

    if encoding.environ.get("HGUNICODEPEDANTRY", False):
        try:
            reload(sys)
            sys.setdefaultencoding("undefined")
        except NameError:
            pass

    # Make available various deps that are either not new enough on the system
    # or not provided by the system.  These include a newer version of IPython
    # for `hg dbsh` and the thrift runtime for the eden extension
    from edenscm.mercurial import thirdparty

    ipypath = os.path.join(os.path.dirname(thirdparty.__file__), "IPython.zip")
    if not os.path.exists(ipypath):
        # we keep the IPython.zip in different location in case of dev builds
        ipypath = os.path.join(libdir, "build", "IPython.zip")
    if ipypath not in sys.path and os.path.exists(ipypath):
        sys.path.insert(0, ipypath)

    from edenscm.mercurial import executionmodel

    executionmodel.setbinaryexecution(binaryexecution)

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
    run(True)
