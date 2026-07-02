# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


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
    # Replace it early, since it may be read in layer modules.
    if sys.stdin is None:
        sys.stdin = open(os.devnull, "r")

    # On Windows, the system time zone setting, if does not match the time
    # zone from the package building machine, can cause pyc to be invalidated
    # in a zip file. Workaround it by bypassing the mtime check.
    if os.name == "nt":
        import zipimport

        zipimport._get_mtime_and_size_of_source = lambda _s, _p: (0, 0)


_fixsys()


# Keep the module clean
del globals()["_fixsys"]


def run(args, fin, fout, ferr, ctx, skipprehooks):
    import sys

    if args is None:
        args = sys.argv

    from . import traceimport

    traceimport.enable()

    # enable demandimport after enabling traceimport
    from . import hgdemandimport

    hgdemandimport.enable()

    # demandimport has side effect on importing dispatch.
    # so 'import dispatch' happens after demandimport
    from . import dispatch

    dispatch.run(args, fin, fout, ferr, ctx, skipprehooks)
