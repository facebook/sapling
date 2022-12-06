# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Standard Library
import abc
import collections

import bindings
from edenscm import node as nodemod, pycompat
from edenscm.graphmod import CHANGESET, GRANDPARENT, PARENT
from edenscm.pycompat import ensurestr


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
    "version heads bookmarks headdates remotebookmarks",
)
NodeInfo = collections.namedtuple(
    "NodeInfo", "node bookmarks parents author date message phase"
)
SmartlogInfo = collections.namedtuple(
    "SmartlogInfo", "dag public draft version timestamp nodeinfos"
)
WorkspaceInfo = collections.namedtuple("WorkspaceInfo", "name archived version")

PUBLICPHASE = "public"


class FakeCtx(object):
    """fake ctx for fake smartlog from fake nodes"""

    def __init__(self, repo, nodeinfo, rev):
        self._nodeinfo = nodeinfo
        self._repo = repo
        self._rev = rev

    def node(self):
        return self._nodeinfo.node

    def parents(self):
        return self._nodeinfo.parents

    def obsolete(self):
        return False

    def invisible(self):
        return False

    def closesbranch(self):
        return False

    def hex(self):
        return nodemod.hex(self._nodeinfo.node)

    def phasestr(self):
        return self._nodeinfo.phase

    def mutable(self):
        return self.phasestr() != PUBLICPHASE

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

    def files(self):
        return []

    def extra(self):
        return {}


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


class SingletonDecorator(object):
    def __init__(self, klass):
        self.klass = klass
        self.instance = None

    def __call__(self, *args, **kwds):
        if not self.instance:
            self.instance = self.klass(*args, **kwds)
        return self.instance


class BaseService(pycompat.ABC):
    @staticmethod
    def _makeemptyreferences(version):
        return References(version, None, None, None, None)

    @classmethod
    def _makereferences(cls, data):
        """Makes a References object from JSON data

        JSON data must represent json serialization of
        //scm/commitcloud/if/CommitCloudService.thrift
        struct ReferencesData

        Result represents struct References from this module
        """
        version = data["version"]
        newheads = [h for h in data.get("heads", [])]
        newbookmarks = {n: v for n, v in data.get("bookmarks", {}).items()}
        headdates = {h: d for h, d in data.get("head_dates", {}).items()}
        newremotebookmarks = cls._decoderemotebookmarks(
            data.get("remote_bookmarks", [])
        )

        return References(
            version,
            newheads,
            newbookmarks,
            headdates,
            newremotebookmarks,
        )

    @staticmethod
    def _makeremotebookmarks(remotebookmarks):
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

    @staticmethod
    def _decoderemotebookmarks(remotebookmarks):
        """Turns a list of thrift remotebookmarks into a dictionary of remote bookmarks"""
        return {
            _joinremotename(book["remote"], book["name"]): book["node"]
            for book in remotebookmarks
        }

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
        oldremotebookmarks,
        newremotebookmarks,
        clientinfo,
    ):
        """Updates the references to a new version.

        If the update was successful, returns `(True, references)`, where
        `references` is a References object containing the new version.

        If the update was not successful, returns `(False, references)`,
        where `references` is a References object containing the current
        version, including its heads and bookmarks.
        """

    @abstractmethod
    def getreferences(self, reponame, workspace, baseversion, clientinfo):
        """Gets the current references if they differ from the base version"""

    @abstractmethod
    def getsmartlog(self, reponame, workspace, repo, flags=[]):
        """Gets the workspace smartlog"""

    @abstractmethod
    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        """Gets the workspace smartlog by version"""

    @abstractmethod
    def getworkspaces(self, reponame, prefix):
        """Gets the list of workspaces for the given prefix"""

    @abstractmethod
    def getworkspace(self, reponame, workspacename):
        """Gets the workspace information if it exists for the given full workspacename name.

        Return type is Optional[WorkspaceInfo]
        """

    @abstractmethod
    def updateworkspacearchive(self, reponame, workspace, archive):
        """Archive or Restore the given workspace"""

    @abstractmethod
    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""

    @abstractmethod
    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""

    @abstractmethod
    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""

    @staticmethod
    def _makesmartloginfo(data):
        """Returns a SmartlogInfo that supports DAG operations like heads, parents,
        roots, ancestors, descendants, etc.
        """
        nodeinfos = _makenodes(data)
        version = data.get("version")
        timestamp = data.get("timestamp")

        public = _getpublic(nodeinfos)

        # Sort public by date. Connect them. Assume they form a linear history.
        # XXX: This can be incorrect if public history is not linear or not
        # sorted by date. However, nodeinfos only have limited information and
        # sort by date is the best effort we can do here.
        public.sort(key=lambda node: (nodeinfos[node].date, node), reverse=True)

        # {node: [parentnode]}
        publicparents = {node: public[i + 1 : i + 2] for i, node in enumerate(public)}

        def getparents(node):
            parents = publicparents.get(node)
            if parents is None:
                parents = [p for p in nodeinfos[node].parents if p in nodeinfos]
            return parents

        dag = bindings.dag.commits.openmemory()
        commits = [(node, getparents(node), b"") for node in sorted(nodeinfos.keys())]
        dag.addcommits(commits)
        dag = dag.dagalgo()
        return SmartlogInfo(
            dag=dag,
            public=public,
            draft=list(dag.all() - public),
            nodeinfos=nodeinfos,
            version=version,
            timestamp=timestamp,
        )

    @staticmethod
    def _makeworkspacesinfo(workspacesinfos):
        return [
            WorkspaceInfo(
                name=ensurestr(workspacesinfo["name"]),
                archived=bool(workspacesinfo["archived"]),
                version=int(workspacesinfo["version"]),
            )
            for workspacesinfo in workspacesinfos["workspaces"]
        ]

    @staticmethod
    def makedagwalker(smartloginfo, repo):
        """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

        This generator function walks the given fake nodes.

        Return firstbranch, dagwalker tuple.
        """

        public = smartloginfo.public
        publicset = set(public)
        dag = smartloginfo.dag.beautify(public)

        def createctx(repo, node):
            return FakeCtx(repo, smartloginfo.nodeinfos[node], node)

        def parentwithstyle(node, p):
            if node not in publicset:
                return (PARENT, p)
            if p in smartloginfo.nodeinfos[node].parents:
                return (PARENT, p)
            return (GRANDPARENT, p)

        def dagwalker():
            for node in dag.all():
                ctx = createctx(repo, node)
                parents = [parentwithstyle(node, p) for p in dag.parentnames(node)]
                yield (node, CHANGESET, ctx, parents)

        firstbranch = public[0:1]
        return firstbranch, dagwalker()

    @staticmethod
    def makedagwalkerwithparents(smartloginfo, repo):
        """cset DAG generator yielding (id, CHANGESET, ctx, [parent ctxs]) tuples

        This generator function walks the given fake nodes.

        Return firstbranch, dagwalker tuple.
        """

        public = smartloginfo.public
        dag = smartloginfo.dag.beautify(public)

        def createctx(repo, node, rev=None):
            return FakeCtx(repo, smartloginfo.nodeinfos[node], rev)

        def dagwalker():
            for node in dag.all():
                ctx = createctx(repo, node, node)
                parents = [createctx(repo, p) for p in dag.parentnames(node)]

                def gen_parents():
                    return parents

                ctx.parents = gen_parents
                yield (node, CHANGESET, ctx, parents)

        firstbranch = public[0:1]
        return firstbranch, dagwalker()


def _makenodes(data):
    nodes = {}
    for nodeinfo in data["nodes"]:
        node = nodemod.bin(ensurestr(nodeinfo["node"]))
        parents = [nodemod.bin(ensurestr(p)) for p in nodeinfo["parents"]]
        bookmarks = [ensurestr(b) for b in nodeinfo["bookmarks"]]
        author = ensurestr(nodeinfo["author"])
        date = int(nodeinfo["date"])
        message = ensurestr(nodeinfo["message"])
        phase = ensurestr(nodeinfo["phase"])
        if "remote_bookmarks" in nodeinfo:
            bookmarks.extend(
                "%s/%s" % (ensurestr(bm["remote"]), ensurestr(bm["name"]))
                for bm in nodeinfo["remote_bookmarks"] or []
            )
        nodes[node] = NodeInfo(node, bookmarks, parents, author, date, message, phase)
    return nodes


def _getpublic(nodeinfos):
    """Get binary public nodes"""
    return [node for node, info in nodeinfos.items() if info.phase == PUBLICPHASE]
