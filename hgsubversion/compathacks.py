"""Functions to work around API changes."""

import errno
import sys

from mercurial import util

def branchset(repo):
    """Return the set of branches present in a repo.

    Works around branchtags() vanishing between 2.8 and 2.9.
    """
    try:
        return set(repo.branchmap())
    except AttributeError:
        return set(repo.branchtags())

def pickle_load(f):
    import cPickle as pickle
    f.seek(0)
    return pickle.load(f)

def makememfilectx(repo, path, data, islink, isexec, copied):
    """Return a memfilectx

    Works around memfilectx() adding a repo argument between 3.0 and 3.1.
    """
    from mercurial import context
    try:
        return context.memfilectx(repo, path, data, islink, isexec, copied)
    except TypeError:
        return context.memfilectx(path, data, islink, isexec, copied)

def filectxfn_deleted(memctx, path):
    """
    Return None or raise an IOError as necessary if path is deleted.

    Call as:

    if path_missing:
        return compathacks.filectxfn_deleted(memctx, path)

    Works around filectxfn's contract changing between 3.1 and 3.2: 3.2 onwards,
    for deleted files, filectxfn should return None rather than returning
    IOError.
    """
    if getattr(memctx, '_returnnoneformissingfiles', False):
        return None
    raise IOError(errno.ENOENT, '%s is deleted' % path)

def filectxfn_deleted_reraise(memctx):
    """
    Return None or reraise exc as necessary.

    Call as:

    try:
        # code that raises IOError if the path is missing
    except IOError:
        return compathacks.filectxfn_deleted_reraise(memctx)

    Works around filectxfn's contract changing between 3.1 and 3.2: 3.2 onwards,
    for deleted files, filectxfn should return None rather than returning
    IOError.
    """
    exc_info = sys.exc_info()
    if (exc_info[1].errno == errno.ENOENT and
        getattr(memctx, '_returnnoneformissingfiles', False)):
        return None
    # preserve traceback info
    raise exc_info[0], exc_info[1], exc_info[2]

# copied from hg 3.8
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
