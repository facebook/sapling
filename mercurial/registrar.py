# registrar.py - utilities to register function for specific purpose
#
#  Copyright FUJIWARA Katsunori <foozy@lares.dti.ne.jp> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import (
    util,
)

class _funcregistrarbase(object):
    """Base of decorator to register a fuction for specific purpose

    This decorator stores decorated functions into own dict 'table'.

    The least derived class can be defined by overriding 'formatdoc',
    for example::

        class keyword(_funcregistrarbase):
            _docformat = ":%s: %s"

    This should be used as below:

        keyword = registrar.keyword()

        @keyword('bar')
        def barfunc(*args, **kwargs):
            '''Explanation of bar keyword ....
            '''
            pass

    In this case:

    - 'barfunc' is stored as 'bar' in '_table' of an instance 'keyword' above
    - 'barfunc.__doc__' becomes ":bar: Explanation of bar keyword"
    """
    def __init__(self, table=None):
        if table is None:
            self._table = {}
        else:
            self._table = table

    def __call__(self, decl, *args, **kwargs):
        return lambda func: self._doregister(func, decl, *args, **kwargs)

    def _doregister(self, func, decl, *args, **kwargs):
        name = self._getname(decl)

        if func.__doc__ and not util.safehasattr(func, '_origdoc'):
            doc = func.__doc__.strip()
            func._origdoc = doc
            func.__doc__ = self._formatdoc(decl, doc)

        self._table[name] = func
        self._extrasetup(name, func, *args, **kwargs)

        return func

    def _parsefuncdecl(self, decl):
        """Parse function declaration and return the name of function in it
        """
        i = decl.find('(')
        if i >= 0:
            return decl[:i]
        else:
            return decl

    def _getname(self, decl):
        """Return the name of the registered function from decl

        Derived class should override this, if it allows more
        descriptive 'decl' string than just a name.
        """
        return decl

    _docformat = None

    def _formatdoc(self, decl, doc):
        """Return formatted document of the registered function for help

        'doc' is '__doc__.strip()' of the registered function.
        """
        return self._docformat % (decl, doc)

    def _extrasetup(self, name, func):
        """Execute exra setup for registered function, if needed
        """
        pass

class revsetpredicate(_funcregistrarbase):
    """Decorator to register revset predicate

    Usage::

        revsetpredicate = registrar.revsetpredicate()

        @revsetpredicate('mypredicate(arg1, arg2[, arg3])')
        def mypredicatefunc(repo, subset, x):
            '''Explanation of this revset predicate ....
            '''
            pass

    The first string argument is used also in online help.

    Optional argument 'safe' indicates whether a predicate is safe for
    DoS attack (False by default).

    'revsetpredicate' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'revsetpredicate' is used for
    decorating in extension.

    Otherwise, explicit 'revset.loadpredicate()' is needed.
    """
    _getname = _funcregistrarbase._parsefuncdecl
    _docformat = "``%s``\n    %s"

    def _extrasetup(self, name, func, safe=False):
        func._safe = safe
