# win32mbcs.py -- MBCS filename support for Mercurial on Windows
#
# Copyright (c) 2008 Shun-ichi Goto <shunichi.goto@gmail.com>
#
# Version: 0.1
# Author:  Shun-ichi Goto <shunichi.goto@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
"""Allow to use shift_jis/big5 filenames on Windows.

There is a well known issue "0x5c problem" on Windows.  It is a
trouble on handling path name as raw encoded byte sequence of
problematic encodings like shift_jis or big5.  The primary intent
of this extension is to allow using such a encoding on Mercurial
without strange file operation error.

By enabling this extension, hook mechanism is activated and some
functions are altered.  Usually, this encoding is your local encoding
on your system by default. So you can get benefit simply by enabling
this extension.

The encoding for filename is same one for terminal by default.  You
can change the encoding by setting HGENCODING environment variable.

This extension is usefull for:
 * Japanese Windows user using shift_jis encoding.
 * Chinese Windows user using big5 encoding.
 * Users who want to use a repository created with such a encoding.

Note: Unix people does not need to use this extension.

"""

import os
from mercurial.i18n import _
from mercurial import util

__all__ = ['install', 'uninstall', 'reposetup']


# codec and alias names of sjis and big5 to be faked.
_problematic_encodings = util.frozenset([
        'big5', 'big5-tw', 'csbig5',
        'big5hkscs', 'big5-hkscs', 'hkscs',
        'cp932', '932', 'ms932', 'mskanji', 'ms-kanji',
        'shift_jis', 'csshiftjis', 'shiftjis', 'sjis', 's_jis',
        'shift_jis_2004', 'shiftjis2004', 'sjis_2004', 'sjis2004',
        'shift_jisx0213', 'shiftjisx0213', 'sjisx0213', 's_jisx0213',
        ])

# attribute name to store original function
_ORIGINAL = '_original'

_ui = None

def decode_with_check(arg):
    if isinstance(arg, tuple):
        return tuple(map(decode_with_check, arg))
    elif isinstance(arg, list):
        return map(decode_with_check, arg)
    elif isinstance(arg, str):
        uarg = arg.decode(util._encoding)
        if arg == uarg.encode(util._encoding):
            return uarg
        else:
            raise UnicodeError("Not local encoding")
    else:
        return arg

def encode_with_check(arg):
    if isinstance(arg, tuple):
        return tuple(map(encode_with_check, arg))
    elif isinstance(arg, list):
        return map(encode_with_check, arg)
    elif isinstance(arg, unicode):
        ret = arg.encode(util._encoding)
        return ret
    else:
        return arg

def wrap(func):

    def wrapped(*args):
        # check argument is unicode, then call original
        for arg in args:
            if isinstance(arg, unicode):
                return func(*args)
        # make decoded argument list into uargs
        try:
            args = decode_with_check(args)
        except UnicodeError, exc:
            # If not encoded with _local_fs_encoding, report it then
            # continue with calling original function.
            _ui.warn(_("WARNING: [win32mbcs] filename conversion fail for" +
                     " %s: '%s'\n") % (util._encoding, args))
            return func(*args)
        # call as unicode operation, then return with encoding
        return encode_with_check(func(*args))

    # fake is only for relevant environment.
    if hasattr(func, _ORIGINAL) or \
            util._encoding.lower() not in _problematic_encodings:
        return func
    else:
        f = wrapped
        f.__name__ = func.__name__
        setattr(f, _ORIGINAL, func)   # hold original to restore
        return f

def unwrap(func):
    return getattr(func, _ORIGINAL, func)

def install():
    # wrap some python functions and mercurial functions
    # to handle raw bytes on Windows.
    # NOTE: dirname and basename is safe because they use result
    # of os.path.split()
    global _ui
    if not _ui:
        from mercurial import ui
        _ui = ui.ui()
    os.path.join = wrap(os.path.join)
    os.path.split = wrap(os.path.split)
    os.path.splitext = wrap(os.path.splitext)
    os.path.splitunc = wrap(os.path.splitunc)
    os.path.normpath = wrap(os.path.normpath)
    os.path.normcase = wrap(os.path.normcase)
    os.makedirs = wrap(os.makedirs)
    util.endswithsep = wrap(util.endswithsep)
    util.splitpath = wrap(util.splitpath)

def uninstall():
    # restore original functions.
    os.path.join = unwrap(os.path.join)
    os.path.split = unwrap(os.path.split)
    os.path.splitext = unwrap(os.path.splitext)
    os.path.splitunc = unwrap(os.path.splitunc)
    os.path.normpath = unwrap(os.path.normpath)
    os.path.normcase = unwrap(os.path.normcase)
    os.makedirs = unwrap(os.makedirs)
    util.endswithsep = unwrap(util.endswithsep)
    util.splitpath = unwrap(util.splitpath)


def reposetup(ui, repo):
    # TODO: decide use of config section for this extension
    global _ui
    _ui = ui
    if not os.path.supports_unicode_filenames:
        ui.warn(_("[win32mbcs] cannot activate on this platform.\n"))
        return
    # install features of this extension
    install()
    ui.debug(_("[win32mbcs] activeted with encoding: %s\n") % util._encoding)

# win32mbcs.py ends here
