from mercurial import util
import weakref

def tolist(val):
    """
    a convenience method to return an empty list instead of None
    """
    if val is None:
        return []
    else:
        return [val]

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

    def __init__(self, repo):
        self._names = util.sortdict()
        self._repo = weakref.ref(repo)

    @property
    def repo(self):
        return self._repo()

    def addnamespace(self, namespace, namemap, order=None):
        """
        register a namespace

        namespace: the name to be registered (in plural form)
        namemap: function that inputs a node, output name(s)
        order: optional argument to specify the order of namespaces
               (e.g. 'branches' should be listed before 'bookmarks')
        """
        val = {'namemap': namemap}
        if order is not None:
            self._names.insert(order, namespace, val)
        else:
            self._names[namespace] = val
