# Copyright Facebook, Inc. 2019
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


def _fixsyspath():
    """Fix sys.path so core edenscm modules (edenscmnative, and 3rd party
    libraries) are in sys.path
    """
    # Do not expose those modules to edenscm.__dict__
    import sys
    import os

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
    for candidate in [libdir, os.path.join(libdir, "build")]:
        depspath = os.path.join(candidate, "edenscmdeps.zip")
        if os.path.exists(depspath) and depspath not in sys.path:
            sys.path.insert(0, depspath)

    # Make sure "edenscmnative" can be imported. Error early.
    import edenscmnative

    edenscmnative.__name__


_fixsyspath()


# Keep the module clean
del globals()["_fixsyspath"]
