# win32mbcs.py -- MBCS filename support for Mercurial
#
# Copyright (c) 2008 Shun-ichi Goto <shunichi.goto@gmail.com>
#
# Version: 0.2
# Author:  Shun-ichi Goto <shunichi.goto@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
"""Allow to use MBCS path with problematic encoding.

Some MBCS encodings are not good for some path operations
(i.e. splitting path, case conversion, etc.) with its encoded bytes.
We call such a encoding (i.e. shift_jis and big5) as "problematic
encoding".  This extension can be used to fix the issue with those
encodings by wrapping some functions to convert to unicode string
before path operation.

This extension is usefull for:
 * Japanese Windows users using shift_jis encoding.
 * Chinese Windows users using big5 encoding.
 * All users who use a repository with one of problematic encodings
   on case-insensitive file system.

This extension is not needed for:
 * Any user who use only ascii chars in path.
 * Any user who do not use any of problematic encodings.

Note that there are some limitations on using this extension:
 * You should use single encoding in one repository.
 * You should set same encoding for the repository by locale or HGENCODING.

To use this extension, enable the extension in .hg/hgrc or ~/.hgrc:

  [extensions]
  hgext.win32mbcs =

Path encoding conversion are done between unicode and util._encoding
which is decided by mercurial from current locale setting or HGENCODING.

"""

import os
from mercurial.i18n import _
from mercurial import util

def decode(arg):
   if isinstance(arg, str):
       uarg = arg.decode(util._encoding)
       if arg == uarg.encode(util._encoding):
           return uarg
       raise UnicodeError("Not local encoding")
   elif isinstance(arg, tuple):
       return tuple(map(decode, arg))
   elif isinstance(arg, list):
       return map(decode, arg)
   return arg

def encode(arg):
   if isinstance(arg, unicode):
       return arg.encode(util._encoding)
   elif isinstance(arg, tuple):
       return tuple(map(encode, arg))
   elif isinstance(arg, list):
       return map(encode, arg)
   return arg

def wrapper(func, args):
   # check argument is unicode, then call original
   for arg in args:
       if isinstance(arg, unicode):
           return func(*args)

   try:
       # convert arguments to unicode, call func, then convert back
       return encode(func(*decode(args)))
   except UnicodeError:
       # If not encoded with util._encoding, report it then
       # continue with calling original function.
      raise util.Abort(_("[win32mbcs] filename conversion fail with"
                         " %s encoding\n") % (util._encoding))

def wrapname(name):
   idx = name.rfind('.')
   module = name[:idx]
   name = name[idx+1:]
   module = eval(module)
   func = getattr(module, name)
   def f(*args):
       return wrapper(func, args)
   try:
      f.__name__ = func.__name__                # fail with python23
   except Exception:
      pass
   setattr(module, name, f)

# List of functions to be wrapped.
# NOTE: os.path.dirname() and os.path.basename() are safe because
#       they use result of os.path.split()
funcs = '''os.path.join os.path.split os.path.splitext
 os.path.splitunc os.path.normpath os.path.normcase os.makedirs
 util.endswithsep util.splitpath util.checkcase util.fspath'''

# codec and alias names of sjis and big5 to be faked.
problematic_encodings = '''big5 big5-tw csbig5 big5hkscs big5-hkscs
 hkscs cp932 932 ms932 mskanji ms-kanji shift_jis csshiftjis shiftjis
 sjis s_jis shift_jis_2004 shiftjis2004 sjis_2004 sjis2004
 shift_jisx0213 shiftjisx0213 sjisx0213 s_jisx0213'''

def reposetup(ui, repo):
   # TODO: decide use of config section for this extension
   if not os.path.supports_unicode_filenames:
       ui.warn(_("[win32mbcs] cannot activate on this platform.\n"))
       return

   # fake is only for relevant environment.
   if util._encoding.lower() in problematic_encodings.split():
       for f in funcs.split():
           wrapname(f)
       ui.debug(_("[win32mbcs] activated with encoding: %s\n") % util._encoding)

