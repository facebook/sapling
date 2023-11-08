# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import


def _fixsys():
    """Fix sys.path so core sapling modules (and 3rd party
    libraries) are in sys.path

    Fix sys.stdin if it's None.
    """
    import os

    # Do not expose those modules to sapling.__dict__
    import sys

    dirname = os.path.dirname

    # __file__ is "hg/sapling/__init__.py"
    # libdir is "hg/"
    # Do not follow symlinks (ex. do not use "realpath"). It breaks buck build.
    libdir = dirname(dirname(os.path.abspath(__file__)))

    # stdin can be None if the parent process unset the stdin file descriptor.
    # Replace it early, since it may be read in layer modules, like pycompat.
    if sys.stdin is None:
        sys.stdin = open(os.devnull, "r")

    # On Windows, the system time zone setting, if does not match the time
    # zone from the package building machine, can cause pyc to be invalidated
    # in a zip file. Workaround it by bypassing the mtime check.
    if os.name == "nt" and sys.version_info[0] == 3:
        import zipimport

        zipimport._get_mtime_and_size_of_source = lambda _s, _p: (0, 0)


_fixsys()


# Keep the module clean
del globals()["_fixsys"]


def run(args=None, fin=None, fout=None, ferr=None, config=None):
    import sys

    if args is None:
        args = sys.argv

    if args[1:2] == ["start-pfc-server"]:
        # chgserver code path

        from . import prefork

        # Set a global so other modules know we are about to fork. They may want
        # to avoid doing/initializing certain things that are not fork safe.
        prefork.prefork = True

        # no demandimport, since chgserver wants to preimport everything.
        from . import dispatch

        dispatch.runchgserver(args[2:])
    else:
        # non-chgserver code path
        # - no chg in use: hgcommands::run -> HgPython::run_hg -> here
        # - chg client: chgserver.runcommand -> bindings.commands.run ->
        #               hgcommands::run -> HgPython::run_hg -> here

        from . import traceimport

        traceimport.enable()

        # enable demandimport after enabling traceimport
        from . import hgdemandimport

        hgdemandimport.enable()

        # demandimport has side effect on importing dispatch.
        # so 'import dispatch' happens after demandimport
        from . import dispatch

        dispatch.run(args, fin, fout, ferr, config)
