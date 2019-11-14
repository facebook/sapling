# Dummy extension to define a namespace containing revision names

from __future__ import absolute_import

from edenscm.mercurial import namespaces, registrar


namespacepredicate = registrar.namespacepredicate()


@namespacepredicate("revnames", priority=70)
def _revnamelookup(repo):
    names = {"r%d" % rev: repo[rev].node() for rev in repo}
    namemap = lambda r, name: names.get(name)
    nodemap = lambda r, node: ["r%d" % repo[node].rev()]

    return namespaces.namespace(
        templatename="revname",
        logname="revname",
        listnames=lambda r: names.keys(),
        namemap=namemap,
        nodemap=nodemap,
    )
