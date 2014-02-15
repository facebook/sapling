"""Functions to work around API changes."""


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
