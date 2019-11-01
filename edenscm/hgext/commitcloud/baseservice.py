# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Standard Library
import abc
import collections
import json

from edenscm.mercurial import dagop, node as nodemod
from edenscm.mercurial.graphmod import CHANGESET, GRANDPARENT, MISSINGPARENT, PARENT


def _joinremotename(remote, name):
    return "/".join([remote, name])


def _splitremotename(remotename):
    name = ""
    if "/" in remotename:
        remote, name = remotename.split("/", 1)
    return remote, name


abstractmethod = abc.abstractmethod
References = collections.namedtuple(
    "References",
    "version heads bookmarks obsmarkers headdates remotebookmarks snapshots",
)
NodeInfo = collections.namedtuple(
    "NodeInfo", "node bookmarks parents author date message phase"
)


class FakeCtx(object):
    """fake ctx for fake smartlog from fake nodes"""

    def __init__(self, repo, nodeinfo, rev):
        self._nodeinfo = nodeinfo
        self._repo = repo
        self._rev = rev

    def node(self):
        return self._nodeinfo.node

    def obsolete(self):
        return False

    def invisible(self):
        return False

    def closesbranch(self):
        return False

    def hex(self):
        return self._nodeinfo.node

    def phasestr(self):
        return self._nodeinfo.phase

    def description(self):
        return self._nodeinfo.message

    def repo(self):
        return self._repo

    def rev(self):
        return self._rev

    def branch(self):
        return "default"

    def bookmarks(self):
        return self._nodeinfo.bookmarks

    def user(self):
        return self._nodeinfo.author

    def date(self):
        return (self._nodeinfo.date, 0)


def topological(graph):
    tovisit = sorted(graph.keys())
    order, state = collections.deque(), {}

    def dfs(node):
        GRAY, BLACK = 0, 1
        state[node] = GRAY
        for k in graph.get(node, ()):
            sk = state.get(k, None)
            if sk == GRAY:
                raise ValueError("cycle detected")
            if sk == BLACK:
                continue
            dfs(k)
        order.appendleft(node)
        state[node] = BLACK

    while tovisit:
        node = tovisit.pop()
        if node not in state:
            dfs(node)
    return order


class BaseService(object):
    __metaclass__ = abc.ABCMeta

    def _makereferences(self, data):
        """Makes a References object from JSON data

            JSON data must represent json serialization of
            //scm/commitcloud/if/CommitCloudService.thrift
            struct ReferencesData

            Result represents struct References from this module
        """
        version = data["version"]
        newheads = [h.encode("ascii") for h in data["heads"]]
        newbookmarks = {
            n.encode("utf-8"): v.encode("ascii") for n, v in data["bookmarks"].items()
        }
        newobsmarkers = [
            (
                nodemod.bin(m["pred"]),
                tuple(nodemod.bin(s) for s in m["succs"]),
                m["flags"],
                tuple(
                    (k.encode("utf-8"), v.encode("utf-8"))
                    for k, v in json.loads(m["meta"])
                ),
                (m["date"], m["tz"]),
                tuple(nodemod.bin(p) for p in m["predparents"]),
            )
            for m in data["new_obsmarkers_data"]
        ]
        headdates = {
            h.encode("ascii"): d for h, d in data.get("head_dates", {}).items()
        }
        newremotebookmarks = {
            _joinremotename(
                book["remote"].encode("utf-8"), book["name"].encode("utf-8")
            ): book["node"].encode("ascii")
            for book in data.get("remote_bookmarks", [])
        }
        newsnapshots = [s.encode("ascii") for s in data["snapshots"]]

        return References(
            version,
            newheads,
            newbookmarks,
            newobsmarkers,
            headdates,
            newremotebookmarks,
            newsnapshots,
        )

    def _encodedmarkers(self, obsmarkers):
        # pred, succs, flags, metadata, date, parents = marker
        return [
            {
                "pred": nodemod.hex(m[0]),
                "succs": [nodemod.hex(s) for s in m[1]],
                "predparents": [nodemod.hex(p) for p in m[5]] if m[5] else [],
                "flags": m[2],
                "date": float(repr(m[4][0])),
                "tz": m[4][1],
                "meta": json.dumps(m[3]),
            }
            for m in obsmarkers
        ]

    def _makeremotebookmarks(self, remotebookmarks):
        """Makes a RemoteBookmark object from dictionary '{remotename: node}'
        or list '[remotename, ...]'.

            Result represents struct RemoteBookmark from
            //scm/commitcloud/if/CommitCloudService.thrift module.
        """
        remotebookslist = []

        def appendremotebook(remotename, node=None):
            remote, name = _splitremotename(remotename)
            remotebook = {"remote": remote, "name": name}
            if node:
                remotebook["node"] = node
            remotebookslist.append(remotebook)

        if type(remotebookmarks) is dict:
            for remotename, node in remotebookmarks.items():
                appendremotebook(remotename, node)
        else:
            for remotename in remotebookmarks:
                appendremotebook(remotename)
        return remotebookslist

    @abstractmethod
    def requiresauthentication(self):
        """Returns True if the service requires authentication tokens"""

    @abstractmethod
    def check(self):
        """Returns True if the connection to the service is ok"""

    @abstractmethod
    def updatereferences(
        self,
        reponame,
        workspace,
        version,
        oldheads,
        newheads,
        oldbookmarks,
        newbookmarks,
        newobsmarkers,
        oldremotebookmarks,
        newremotebookmarks,
        oldsnapshots,
        newsnapshots,
    ):
        """Updates the references to a new version.

        If the update was successful, returns `(True, references)`, where
        `references` is a References object containing the new version.

        If the update was not successful, returns `(False, references)`,
        where `references` is a References object containing the current
        version, including its heads and bookmarks.
        """

    @abstractmethod
    def getreferences(self, reponame, workspace, baseversion):
        """Gets the current references if they differ from the base version
        """

    @abstractmethod
    def getsmartlog(self, reponame, workspace, repo):
        """Gets the workspace smartlog
        """

    def _makefakedag(self, nodeinfos, repo):
        """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

        This generator function walks the given fake nodes.
        """

        if not nodeinfos:
            return []

        DRAFTPHASE = "draft"

        parentchildmap = {}

        ##### HELPER FUNCTIONS #####

        def sortbydate(listofnodes):
            # also sort by node to disambiguate the ordering with the same date
            return sorted(
                listofnodes, key=lambda node: (nodeinfos[node].date, node), reverse=True
            )

        def isfinalnode(node):
            return node not in allnodes or all(
                p not in allnodes for p in nodeinfos[node].parents
            )

        def isdraftnode(node):
            return node in nodeinfos and nodeinfos[node].phase == DRAFTPHASE

        def ispublicnode(node):
            return not isdraftnode(node)

        def publicpathtop(publicnode):
            """returns the top-most node in public nodes path for the given node"""

            # for the example below returns 028179 for the given node 33254d
            #
            #  o  028179  (public) Jun 29 at 11:30
            #  |
            #  o  33254d  (public) Jun 28 at 09:45

            while True:
                nodechildlist = [
                    p for p in parentchildmap.get(publicnode, []) if ispublicnode(p)
                ]
                if not nodechildlist:
                    break
                publicnode = nodechildlist[0]
            return publicnode

        ##### HELPER FUNCTIONS END #####

        # set of all nodes (excluding their parents)
        allnodes = set(nodeinfos.keys())

        # Initial parent child map
        for n in sorted(allnodes):
            for p in nodeinfos[n].parents:
                parentchildmap.setdefault(p, []).append(n)

        # originally data is a set of trees where draft stacks teminate with a public node
        # connect these trees is the first step

        # select nodes that don't have parents present in all nodes
        # let's call them 'final nodes'

        finalnodes = [n for n in allnodes if isfinalnode(n)]

        # sort the final nodes by date
        # glue them and add these edges to the grapth
        #
        # for the example below on this pass the following additional edges will be added:
        #
        #    o  2c008f  (draft) Jun 29 at 11:33     (none)
        #   /   some commit
        #  |
        #  o  028179  (public) Jun 29 at 11:30      (edge this node ->  33254d)
        #  |
        #  | o  7c2d07  (draft) Jun 28 at 14:25     (none)
        #  |/   some commit
        #  |
        #  o  33254d  (public) Jun 28 at 09:45

        # XXX: This adds faked edges. Practically finalnodes are usually public
        # nodes that exist in the repo. A better approach is to check the real
        # repo to figure out the real edges of them, and do not add faked edges.
        # Ideally, grandparent edges and direct parent edges can be
        # distinguished that way.
        finalnodes = sortbydate(finalnodes)

        for i, node in enumerate(finalnodes[:-1]):
            nextnode = finalnodes[i + 1]
            gluenode = publicpathtop(nextnode)
            parentchildmap.setdefault(gluenode, []).append(node)

        # Build the reversed map. Useful for "parentrevs" used by
        # "dagop.topsort".
        childparentmap = {}
        for parent, children in sorted(parentchildmap.items()):
            for child in children:
                childparentmap.setdefault(child, []).append(parent)

        # Add missing nodes
        for info in nodeinfos.values():
            for node in [info.node] + info.parents:
                childparentmap.setdefault(node, [])
                parentchildmap.setdefault(node, [])

        # Assign revision numbers. Useful for functions like "dagop.topsort".
        revnodemap = {}
        noderevmap = {}
        for i, node in enumerate(topological(parentchildmap)):
            rev = 1000000000 + i
            revnodemap[rev] = node
            noderevmap[node] = rev

        # Replacement of repo.changelog.parentrevs
        def parentrevs(rev):
            node = revnodemap[rev]
            result = [noderevmap[n] for n in childparentmap[node]]
            return result

        # Set "first branch" to "finalnodes". They are usually public commits.
        firstbranch = set([noderevmap[finalnodes[0]]])
        repo.ui.debug("building dag: firstbranch: %r" % finalnodes[0])

        # Use "dagop.toposort" to sort them. This helps beautify the graph.
        allrevs = sorted(noderevmap[n] for n in allnodes)
        sortedrevs = list(dagop.toposort(allrevs, parentrevs, firstbranch))

        def createctx(repo, node):
            return FakeCtx(repo, nodeinfos[node], noderevmap[node])

        # Copied from graphmod.dagwalker. Revised.
        def dagwalker(repo, revs):
            """cset DAG generator yielding (id, CHANGESET, ctx, [parentinfo]) tuples

            This generator function walks through revisions (which should be ordered
            from bigger to lower). It returns a tuple for each node.

            Each parentinfo entry is a tuple with (edgetype, parentid), where edgetype
            is one of PARENT, GRANDPARENT or MISSINGPARENT. The node and parent ids
            are arbitrary integers which identify a node in the context of the graph
            returned.

            """
            minroot = min(revs)
            gpcache = {}

            for rev in revs:
                node = revnodemap[rev]
                ctx = createctx(repo, node)
                # TODO: Consider generating faked nodes (missing parents) for
                # missing parents.
                parentctxs = [
                    createctx(repo, n) for n in childparentmap[node] if n in nodeinfos
                ]
                # partition into parents in the rev set and missing parents, then
                # augment the lists with markers, to inform graph drawing code about
                # what kind of edge to draw between nodes.
                pset = set(p.rev() for p in parentctxs if p.rev() in revs)
                mpars = [p.rev() for p in parentctxs if p.rev() not in pset]
                # Heuristic: finalnodes only have grandparents
                if node in finalnodes:
                    parentstyle = GRANDPARENT
                else:
                    parentstyle = PARENT
                parents = [(parentstyle, p) for p in sorted(pset)]

                for mpar in mpars:
                    gp = gpcache.get(mpar)
                    if gp is None:
                        gp = gpcache[mpar] = sorted(
                            set(
                                dagop._reachablerootspure(
                                    repo,
                                    minroot,
                                    revs,
                                    [mpar],
                                    False,
                                    parentrevs=parentrevs,
                                )
                            )
                        )
                    if not gp:
                        parents.append((MISSINGPARENT, mpar))
                        pset.add(mpar)
                    else:
                        parents.extend((GRANDPARENT, g) for g in gp if g not in pset)
                        pset.update(gp)

                yield (ctx.rev(), CHANGESET, ctx, parents)

        return dagwalker(repo, sortedrevs)

    def _makenodes(self, data):
        nodes = {}
        for nodeinfo in data["nodes"]:
            node = nodeinfo["node"].encode("ascii")
            parents = [p.encode("ascii") for p in nodeinfo["parents"]]
            bookmarks = [b.encode("utf-8") for b in nodeinfo["bookmarks"]]
            author = nodeinfo["author"].encode("utf-8")
            date = int(nodeinfo["date"])
            message = nodeinfo["message"].encode("utf-8")
            phase = nodeinfo["phase"].encode("utf-8")
            nodes[node] = NodeInfo(
                node, bookmarks, parents, author, date, message, phase
            )
        return nodes
