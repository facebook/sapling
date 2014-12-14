from mercurial import util

class namespaces(object):
    """
    provides an interface to register a generic many-to-many mapping between
    some (namespaced) names and nodes. The goal here is to control the
    pollution of jamming things into tags or bookmarks (in extension-land) and
    to simplify internal bits of mercurial: log output, tab completion, etc.

    More precisely, we define a list of names (the namespace) and  a mapping of
    names to nodes. This name mapping returns a list of nodes.

    Furthermore, each name mapping will be passed a name to lookup which might
    not be in its domain. In this case, each method should return an empty list
    and not raise an error.

    We'll have a dictionary '_names' where each key is a namespace and
    its value is a dictionary of functions:
      'namemap': function that takes a name and returns a list of nodes
    """

    _names_version = 0

    def __init__(self):
        self._names = util.sortdict()
