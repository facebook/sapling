"""Functions to work around API changes inside Mercurial."""

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
