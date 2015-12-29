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

class funcregistrar(object):
    """Base of decorator to register a fuction for specific purpose

    The least derived class can be defined by overriding 'table' and
    'formatdoc', for example::

        symbols = {}
        class keyword(funcregistrar):
            table = symbols
            formatdoc = ":%s: %s"

        @keyword('bar')
        def barfunc(*args, **kwargs):
            '''Explanation of bar keyword ....
            '''
            pass

    In this case:

    - 'barfunc' is registered as 'bar' in 'symbols'
    - online help uses ":bar: Explanation of bar keyword"
    """

    def __init__(self, decl):
        """'decl' is a name or more descriptive string of a function

        Specification of 'decl' depends on registration purpose.
        """
        self.decl = decl

    table = None

    def __call__(self, func):
        """Execute actual registration for specified function
        """
        name = self.getname()

        if func.__doc__ and not util.safehasattr(func, '_origdoc'):
            doc = func.__doc__.strip()
            func._origdoc = doc
            if callable(self.formatdoc):
                func.__doc__ = self.formatdoc(doc)
            else:
                # convenient shortcut for simple format
                func.__doc__ = self.formatdoc % (self.decl, doc)

        self.table[name] = func
        self.extraaction(name, func)

        return func

    def getname(self):
        """Return the name of the registered function from self.decl

        Derived class should override this, if it allows more
        descriptive 'decl' string than just a name.
        """
        return self.decl

    def parsefuncdecl(self):
        """Parse function declaration and return the name of function in it
        """
        i = self.decl.find('(')
        if i > 0:
            return self.decl[:i]
        else:
            return self.decl

    def formatdoc(self, doc):
        """Return formatted document of the registered function for help

        'doc' is '__doc__.strip()' of the registered function.

        If this is overridden by non-callable object in derived class,
        such value is treated as "format string" and used to format
        document by 'self.formatdoc % (self.decl, doc)' for convenience.
        """
        raise NotImplementedError()

    def extraaction(self, name, func):
        """Execute exra action for registered function, if needed
        """
        pass

class delayregistrar(object):
    """Decorator to delay actual registration until uisetup or so

    For example, the decorator class to delay registration by
    'keyword' funcregistrar can be defined as below::

        class extkeyword(delayregistrar):
            registrar = keyword
    """
    def __init__(self):
        self._list = []

    registrar = None

    def __call__(self, *args, **kwargs):
        """Return the decorator to delay actual registration until setup
        """
        assert self.registrar is not None
        def decorator(func):
            # invocation of self.registrar() here can detect argument
            # mismatching immediately
            self._list.append((func, self.registrar(*args, **kwargs)))
            return func
        return decorator

    def setup(self):
        """Execute actual registration
        """
        while self._list:
            func, decorator = self._list.pop(0)
            decorator(func)
