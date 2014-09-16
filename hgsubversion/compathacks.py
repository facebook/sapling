"""Functions to work around API changes."""

import errno
import sys

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
