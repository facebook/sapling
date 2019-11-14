# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2014 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import error, registrar, templatekw, util
from .i18n import _


namespacetable = util.sortdict()


def tolist(val):
    """
    a convenience method to return an empty list instead of None
    """
    if val is None:
        return []
    else:
        return [val]


# Do not use builtinnamespace in extension code. Use `registrar.namespacetable`
# instead.
builtinnamespace = registrar.namespacepredicate(namespacetable)


@builtinnamespace("bookmarks", priority=10)
def bookmarks(repo):
    bmknames = lambda repo: repo._bookmarks.keys()
    bmknamemap = lambda repo, name: tolist(repo._bookmarks.get(name))
    bmknodemap = lambda repo, node: repo.nodebookmarks(node)
    return namespace(
        templatename="bookmark",
        logfmt=templatekw.getlogcolumns()["bookmark"],
        listnames=bmknames,
        namemap=bmknamemap,
        nodemap=bmknodemap,
        builtin=True,
    )


@builtinnamespace("tags", priority=20)
def tags(repo):
    tagnames = lambda repo: [t for t, n in repo.tagslist()]
    tagnamemap = lambda repo, name: tolist(repo._tagscache.tags.get(name))
    tagnodemap = lambda repo, node: repo.nodetags(node)
    return namespace(
        templatename="tag",
        logfmt=templatekw.getlogcolumns()["tag"],
        listnames=tagnames,
        namemap=tagnamemap,
        nodemap=tagnodemap,
        deprecated={"tip"},
        builtin=True,
    )


@builtinnamespace("branches", priority=30)
def branches(repo):
    bnames = lambda repo: repo.branchmap().keys()
    bnamemap = lambda repo, name: tolist(repo.branchtip(name, True))
    bnodemap = lambda repo, node: [repo[node].branch()]
    return namespace(
        templatename="branch",
        logfmt=templatekw.getlogcolumns()["branch"],
        listnames=bnames,
        namemap=bnamemap,
        nodemap=bnodemap,
        builtin=True,
    )


class namespaces(object):
    """provides an interface to register and operate on multiple namespaces. See
    the namespace class below for details on the namespace object.

    """

    _names_version = 0

    def __init__(self, repo):
        self._names = util.sortdict()

        # Insert namespaces specified in the namespacetable, sorted
        # by priority.
        def sortkey(tup):
            name, func = tup
            return (func._priority, name)

        for name, func in sorted(namespacetable.items(), key=sortkey):
            ns = func(repo)
            if ns is not None:
                self._addnamespace(name, ns)

    def __getitem__(self, namespace):
        """returns the namespace object"""
        return self._names[namespace]

    def __iter__(self):
        return self._names.__iter__()

    def items(self):
        return self._names.iteritems()

    iteritems = items

    def _addnamespace(self, name, namespace):
        """register a namespace

        name: the name to be registered (in plural form)
        namespace: namespace to be registered
        """
        self._names[name] = namespace

        # we only generate a template keyword if one does not already exist
        if name not in templatekw.keywords:

            def generatekw(**args):
                return templatekw.shownames(name, **args)

            templatekw.keywords[name] = generatekw

    def singlenode(self, repo, name):
        """
        Return the 'best' node for the given name. Best means the first node
        in the first nonempty list returned by a name-to-nodes mapping function
        in the defined precedence order.

        Raises a KeyError if there is no such node.
        """
        for ns, v in self._names.iteritems():
            # Fast path: do not consider branches unless it's "default".
            if ns == "branches" and name != "default":
                continue
            n = v.namemap(repo, name)
            if n:
                v.accessed(repo, name)
                # return max revision number
                if len(n) > 1:
                    cl = repo.changelog
                    maxrev = max(cl.rev(node) for node in n)
                    return cl.node(maxrev)
                return n[0]
        raise KeyError(_("no such name: %s") % name)


class namespace(object):
    """provides an interface to a namespace

    Namespaces are basically generic many-to-many mapping between some
    (namespaced) names and nodes. The goal here is to control the pollution of
    jamming things into tags or bookmarks (in extension-land) and to simplify
    internal bits of mercurial: log output, tab completion, etc.

    More precisely, we define a mapping of names to nodes, and a mapping from
    nodes to names. Each mapping returns a list.

    Furthermore, each name mapping will be passed a name to lookup which might
    not be in its domain. In this case, each method should return an empty list
    and not raise an error.

    This namespace object will define the properties we need:
      'name': the namespace (plural form)
      'templatename': name to use for templating (usually the singular form
                      of the plural namespace name)
      'listnames': list of all names in the namespace (usually the keys of a
                   dictionary)
      'namemap': function that takes a name and returns a list of nodes
      'nodemap': function that takes a node and returns a list of names
      'deprecated': set of names to be masked for ordinary use
      'builtin': bool indicating if this namespace is supported by core
                 Mercurial.
      'accessed': function, that is used to log if the name from the namespace
                  was accessed. The method helps to build metrics around name
                  "access" event.
    """

    def __init__(
        self,
        templatename=None,
        logname=None,
        colorname=None,
        logfmt=None,
        listnames=None,
        namemap=None,
        nodemap=None,
        deprecated=None,
        builtin=False,
        accessed=None,
    ):
        """create a namespace

        templatename: the name to use for templating
        logname: the name to use for log output; if not specified templatename
                 is used
        colorname: the name to use for colored log output; if not specified
                   logname is used
        logfmt: the format to use for (i18n-ed) log output; if not specified
                it is composed from logname
        listnames: function to list all names
        namemap: function that inputs a name, output node(s)
        nodemap: function that inputs a node, output name(s)
        deprecated: set of names to be masked for ordinary use
        builtin: whether namespace is implemented by core Mercurial
        accessed: function, that is used to log if the name from the namespace
        was accessed

        """
        self.templatename = templatename
        self.logname = logname
        self.colorname = colorname
        self.logfmt = logfmt
        self.listnames = listnames
        self.namemap = namemap
        self.nodemap = nodemap

        if accessed is not None:
            self.accessed = accessed
        else:
            self.accessed = lambda repo, name: None

        # if logname is not specified, use the template name as backup
        if self.logname is None:
            self.logname = self.templatename

        # if colorname is not specified, just use the logname as a backup
        if self.colorname is None:
            self.colorname = self.logname

        # if logfmt is not specified, compose it from logname as backup
        if self.logfmt is None:
            # i18n: column positioning for "hg log"
            self.logfmt = ("%s:" % self.logname).ljust(13) + "%s\n"

        if deprecated is None:
            self.deprecated = set()
        else:
            self.deprecated = deprecated

        self.builtin = builtin

    def names(self, repo, node):
        """method that returns a (sorted) list of names in a namespace that
        match a given node"""
        return sorted(self.nodemap(repo, node))

    def nodes(self, repo, name):
        """method that returns a list of nodes in a namespace that
        match a given name.

        """
        return sorted(self.namemap(repo, name))


def loadpredicate(ui, extname, registrarobj):
    for name, ns in registrarobj._table.iteritems():
        if name in namespacetable:
            raise error.ProgrammingError("namespace '%s' is already registered", name)
        namespacetable[name] = ns
