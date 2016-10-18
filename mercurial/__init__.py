# __init__.py - Startup and module loading logic for Mercurial.
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import imp
import os
import sys
import zipimport

from . import (
    policy
)

__all__ = []

modulepolicy = policy.policy

# Modules that have both Python and C implementations. See also the
# set of .py files under mercurial/pure/.
_dualmodules = set([
    'mercurial.base85',
    'mercurial.bdiff',
    'mercurial.diffhelpers',
    'mercurial.mpatch',
    'mercurial.osutil',
    'mercurial.parsers',
])

class hgimporter(object):
    """Object that conforms to import hook interface defined in PEP-302."""
    def find_module(self, name, path=None):
        # We only care about modules that have both C and pure implementations.
        if name in _dualmodules:
            return self
        return None

    def load_module(self, name):
        mod = sys.modules.get(name, None)
        if mod:
            return mod

        mercurial = sys.modules['mercurial']

        # The zip importer behaves sufficiently differently from the default
        # importer to warrant its own code path.
        loader = getattr(mercurial, '__loader__', None)
        if isinstance(loader, zipimport.zipimporter):
            def ziploader(*paths):
                """Obtain a zipimporter for a directory under the main zip."""
                path = os.path.join(loader.archive, *paths)
                zl = sys.path_importer_cache.get(path)
                if not zl:
                    zl = zipimport.zipimporter(path)
                return zl

            try:
                if modulepolicy in policy.policynoc:
                    raise ImportError()

                zl = ziploader('mercurial')
                mod = zl.load_module(name)
                # Unlike imp, ziploader doesn't expose module metadata that
                # indicates the type of module. So just assume what we found
                # is OK (even though it could be a pure Python module).
            except ImportError:
                if modulepolicy == 'c':
                    raise
                zl = ziploader('mercurial', 'pure')
                mod = zl.load_module(name)

            sys.modules[name] = mod
            return mod

        # Unlike the default importer which searches special locations and
        # sys.path, we only look in the directory where "mercurial" was
        # imported from.

        # imp.find_module doesn't support submodules (modules with ".").
        # Instead you have to pass the parent package's __path__ attribute
        # as the path argument.
        stem = name.split('.')[-1]

        try:
            if modulepolicy in policy.policynoc:
                raise ImportError()

            modinfo = imp.find_module(stem, mercurial.__path__)

            # The Mercurial installer used to copy files from
            # mercurial/pure/*.py to mercurial/*.py. Therefore, it's possible
            # for some installations to have .py files under mercurial/*.
            # Loading Python modules when we expected C versions could result
            # in a) poor performance b) loading a version from a previous
            # Mercurial version, potentially leading to incompatibility. Either
            # scenario is bad. So we verify that modules loaded from
            # mercurial/* are C extensions. If the current policy allows the
            # loading of .py modules, the module will be re-imported from
            # mercurial/pure/* below.
            if modinfo[2][2] != imp.C_EXTENSION:
                raise ImportError('.py version of %s found where C '
                                  'version should exist' % name)

        except ImportError:
            if modulepolicy == 'c':
                raise

            # Could not load the C extension and pure Python is allowed. So
            # try to load them.
            from . import pure
            modinfo = imp.find_module(stem, pure.__path__)
            if not modinfo:
                raise ImportError('could not find mercurial module %s' %
                                  name)

        mod = imp.load_module(name, *modinfo)
        sys.modules[name] = mod
        return mod

# Python 3 uses a custom module loader that transforms source code between
# source file reading and compilation. This is done by registering a custom
# finder that changes the spec for Mercurial modules to use a custom loader.
if sys.version_info[0] >= 3:
    from . import pure
    import importlib
    import io
    import token
    import tokenize

    class hgpathentryfinder(importlib.abc.MetaPathFinder):
        """A sys.meta_path finder that uses a custom module loader."""
        def find_spec(self, fullname, path, target=None):
            # Only handle Mercurial-related modules.
            if not fullname.startswith(('mercurial.', 'hgext.', 'hgext3rd.')):
                return None

            # This assumes Python 3 doesn't support loading C modules.
            if fullname in _dualmodules:
                stem = fullname.split('.')[-1]
                fullname = 'mercurial.pure.%s' % stem
                target = pure
                assert len(path) == 1
                path = [os.path.join(path[0], 'pure')]

            # Try to find the module using other registered finders.
            spec = None
            for finder in sys.meta_path:
                if finder == self:
                    continue

                spec = finder.find_spec(fullname, path, target=target)
                if spec:
                    break

            # This is a Mercurial-related module but we couldn't find it
            # using the previously-registered finders. This likely means
            # the module doesn't exist.
            if not spec:
                return None

            if fullname.startswith('mercurial.pure.'):
                spec.name = spec.name.replace('.pure.', '.')

            # TODO need to support loaders from alternate specs, like zip
            # loaders.
            spec.loader = hgloader(spec.name, spec.origin)
            return spec

    def replacetokens(tokens, fullname):
        """Transform a stream of tokens from raw to Python 3.

        It is called by the custom module loading machinery to rewrite
        source/tokens between source decoding and compilation.

        Returns a generator of possibly rewritten tokens.

        The input token list may be mutated as part of processing. However,
        its changes do not necessarily match the output token stream.

        REMEMBER TO CHANGE ``BYTECODEHEADER`` WHEN CHANGING THIS FUNCTION
        OR CACHED FILES WON'T GET INVALIDATED PROPERLY.
        """
        futureimpline = False

        # The following utility functions access the tokens list and i index of
        # the for i, t enumerate(tokens) loop below
        def _isop(j, *o):
            """Assert that tokens[j] is an OP with one of the given values"""
            try:
                return tokens[j].type == token.OP and tokens[j].string in o
            except IndexError:
                return False

        def _findargnofcall(n):
            """Find arg n of a call expression (start at 0)

            Returns index of the first token of that argument, or None if
            there is not that many arguments.

            Assumes that token[i + 1] is '('.

            """
            nested = 0
            for j in range(i + 2, len(tokens)):
                if _isop(j, ')', ']', '}'):
                    # end of call, tuple, subscription or dict / set
                    nested -= 1
                    if nested < 0:
                        return None
                elif n == 0:
                    # this is the starting position of arg
                    return j
                elif _isop(j, '(', '[', '{'):
                    nested += 1
                elif _isop(j, ',') and nested == 0:
                    n -= 1

            return None

        def _ensureunicode(j):
            """Make sure the token at j is a unicode string

            This rewrites a string token to include the unicode literal prefix
            so the string transformer won't add the byte prefix.

            Ignores tokens that are not strings. Assumes bounds checking has
            already been done.

            """
            st = tokens[j]
            if st.type == token.STRING and st.string.startswith(("'", '"')):
                tokens[j] = st._replace(string='u%s' % st.string)

        for i, t in enumerate(tokens):
            # Convert most string literals to byte literals. String literals
            # in Python 2 are bytes. String literals in Python 3 are unicode.
            # Most strings in Mercurial are bytes and unicode strings are rare.
            # Rather than rewrite all string literals to use ``b''`` to indicate
            # byte strings, we apply this token transformer to insert the ``b``
            # prefix nearly everywhere.
            if t.type == token.STRING:
                s = t.string

                # Preserve docstrings as string literals. This is inconsistent
                # with regular unprefixed strings. However, the
                # "from __future__" parsing (which allows a module docstring to
                # exist before it) doesn't properly handle the docstring if it
                # is b''' prefixed, leading to a SyntaxError. We leave all
                # docstrings as unprefixed to avoid this. This means Mercurial
                # components touching docstrings need to handle unicode,
                # unfortunately.
                if s[0:3] in ("'''", '"""'):
                    yield t
                    continue

                # If the first character isn't a quote, it is likely a string
                # prefixing character (such as 'b', 'u', or 'r'. Ignore.
                if s[0] not in ("'", '"'):
                    yield t
                    continue

                # String literal. Prefix to make a b'' string.
                yield t._replace(string='b%s' % t.string)
                continue

            # Insert compatibility imports at "from __future__ import" line.
            # No '\n' should be added to preserve line numbers.
            if (t.type == token.NAME and t.string == 'import' and
                all(u.type == token.NAME for u in tokens[i - 2:i]) and
                [u.string for u in tokens[i - 2:i]] == ['from', '__future__']):
                futureimpline = True
            if t.type == token.NEWLINE and futureimpline:
                futureimpline = False
                if fullname == 'mercurial.pycompat':
                    yield t
                    continue
                r, c = t.start
                l = (b'; from mercurial.pycompat import '
                     b'delattr, getattr, hasattr, setattr, xrange\n')
                for u in tokenize.tokenize(io.BytesIO(l).readline):
                    if u.type in (tokenize.ENCODING, token.ENDMARKER):
                        continue
                    yield u._replace(
                        start=(r, c + u.start[1]), end=(r, c + u.end[1]))
                continue

            # This looks like a function call.
            if t.type == token.NAME and _isop(i + 1, '('):
                fn = t.string

                # *attr() builtins don't accept byte strings to 2nd argument.
                if (fn in ('getattr', 'setattr', 'hasattr', 'safehasattr') and
                        not _isop(i - 1, '.')):
                    arg1idx = _findargnofcall(1)
                    if arg1idx is not None:
                        _ensureunicode(arg1idx)

                # .encode() and .decode() on str/bytes/unicode don't accept
                # byte strings on Python 3.
                elif fn in ('encode', 'decode') and _isop(i - 1, '.'):
                    for argn in range(2):
                        argidx = _findargnofcall(argn)
                        if argidx is not None:
                            _ensureunicode(argidx)

                # Bare open call (not an attribute on something else), the
                # second argument (mode) must be a string, not bytes
                elif fn == 'open' and not _isop(i - 1, '.'):
                    arg1idx = _findargnofcall(1)
                    if arg1idx is not None:
                        _ensureunicode(arg1idx)

                # It changes iteritems to items as iteritems is not
                # present in Python 3 world.
                elif fn == 'iteritems':
                    yield t._replace(string='items')
                    continue

            # Emit unmodified token.
            yield t

    # Header to add to bytecode files. This MUST be changed when
    # ``replacetoken`` or any mechanism that changes semantics of module
    # loading is changed. Otherwise cached bytecode may get loaded without
    # the new transformation mechanisms applied.
    BYTECODEHEADER = b'HG\x00\x06'

    class hgloader(importlib.machinery.SourceFileLoader):
        """Custom module loader that transforms source code.

        When the source code is converted to a code object, we transform
        certain patterns to be Python 3 compatible. This allows us to write code
        that is natively Python 2 and compatible with Python 3 without
        making the code excessively ugly.

        We do this by transforming the token stream between parse and compile.

        Implementing transformations invalidates caching assumptions made
        by the built-in importer. The built-in importer stores a header on
        saved bytecode files indicating the Python/bytecode version. If the
        version changes, the cached bytecode is ignored. The Mercurial
        transformations could change at any time. This means we need to check
        that cached bytecode was generated with the current transformation
        code or there could be a mismatch between cached bytecode and what
        would be generated from this class.

        We supplement the bytecode caching layer by wrapping ``get_data``
        and ``set_data``. These functions are called when the
        ``SourceFileLoader`` retrieves and saves bytecode cache files,
        respectively. We simply add an additional header on the file. As
        long as the version in this file is changed when semantics change,
        cached bytecode should be invalidated when transformations change.

        The added header has the form ``HG<VERSION>``. That is a literal
        ``HG`` with 2 binary bytes indicating the transformation version.
        """
        def get_data(self, path):
            data = super(hgloader, self).get_data(path)

            if not path.endswith(tuple(importlib.machinery.BYTECODE_SUFFIXES)):
                return data

            # There should be a header indicating the Mercurial transformation
            # version. If it doesn't exist or doesn't match the current version,
            # we raise an OSError because that is what
            # ``SourceFileLoader.get_code()`` expects when loading bytecode
            # paths to indicate the cached file is "bad."
            if data[0:2] != b'HG':
                raise OSError('no hg header')
            if data[0:4] != BYTECODEHEADER:
                raise OSError('hg header version mismatch')

            return data[4:]

        def set_data(self, path, data, *args, **kwargs):
            if path.endswith(tuple(importlib.machinery.BYTECODE_SUFFIXES)):
                data = BYTECODEHEADER + data

            return super(hgloader, self).set_data(path, data, *args, **kwargs)

        def source_to_code(self, data, path):
            """Perform token transformation before compilation."""
            buf = io.BytesIO(data)
            tokens = tokenize.tokenize(buf.readline)
            data = tokenize.untokenize(replacetokens(list(tokens), self.name))
            # Python's built-in importer strips frames from exceptions raised
            # for this code. Unfortunately, that mechanism isn't extensible
            # and our frame will be blamed for the import failure. There
            # are extremely hacky ways to do frame stripping. We haven't
            # implemented them because they are very ugly.
            return super(hgloader, self).source_to_code(data, path)

# We automagically register our custom importer as a side-effect of loading.
# This is necessary to ensure that any entry points are able to import
# mercurial.* modules without having to perform this registration themselves.
if sys.version_info[0] >= 3:
    _importercls = hgpathentryfinder
else:
    _importercls = hgimporter
if not any(isinstance(x, _importercls) for x in sys.meta_path):
    # meta_path is used before any implicit finders and before sys.path.
    sys.meta_path.insert(0, _importercls())
