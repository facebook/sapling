# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import


def _fixsys():
    """Fix sys.path so core edenscm modules (edenscmnative, and 3rd party
    libraries) are in sys.path

    Fix sys.stdin if it's None.
    """
    import os

    # Do not expose those modules to edenscm.__dict__
    import sys

    dirname = os.path.dirname

    # __file__ is "hg/edenscm/__init__.py"
    # libdir is "hg/"
    # Do not follow symlinks (ex. do not use "realpath"). It breaks buck build.
    libdir = dirname(dirname(os.path.abspath(__file__)))

    # Make "edenscmdeps.zip" available in sys.path. It includes 3rd party
    # pure-Python libraries like IPython, thrift runtime, etc.
    #
    # Note: On Windows, the released version of hg uses python27.zip for all
    # pure Python modules including edenscm and everything in edenscmdeps.zip,
    # so not being able to locate edenscmdeps.zip is not fatal.
    if sys.version_info[0] == 3:
        name = "edenscmdeps3.zip"
    else:
        name = "edenscmdeps.zip"
    for candidate in [libdir, os.path.join(libdir, "build")]:
        depspath = os.path.join(candidate, name)
        if os.path.exists(depspath) and depspath not in sys.path:
            sys.path.insert(0, depspath)

    # Make sure "edenscmnative" can be imported. Error early.
    import edenscmnative

    edenscmnative.__name__

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


def run(args=None, fin=None, fout=None, ferr=None):
    import sys

    if args is None:
        args = sys.argv

    if args[1:4] == ["serve", "--cmdserver", "chgunix2"]:
        # chgserver code path

        # Disable tracing as early as possible. Any use of Rust
        # tracing pre-fork messes things up post-fork.
        from . import tracing

        tracing.disabletracing = True

        # no demandimport, since chgserver wants to preimport everything.
        from .mercurial import dispatch

        dispatch.runchgserver()
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
        from .mercurial import dispatch

        dispatch.run(args, fin, fout, ferr)
