import os, errno, sys, time, datetime, pickle, copy
import dulwich
from dulwich.repo import Repo
from dulwich.client import SimpleFetchGraphWalker
from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid
from mercurial import hg, util, context, error

class GitHandler(object):
    
    def __init__(self, dest_repo, ui):
        self.repo = dest_repo
        self.ui = ui
        self.load_git()
        self.load_map()
        
    def load_git(self):
        git_dir = os.path.join(self.repo.path, 'git')
        self.git = Repo(git_dir)

    def load_map(self):
        self._map = {}
        if os.path.exists(self.repo.join('git-mapfile')):
            for line in self.repo.opener('git-mapfile'):
                gitsha, hgsha = line.strip().split(' ', 1)
                self._map[gitsha] = hgsha
            
    def save_map(self):
        file = self.repo.opener('git-mapfile', 'w+')
        for gitsha, hgsha in self._map.iteritems():
            file.write("%s %s\n" % (gitsha, hgsha))
        file.close()

    def fetch(self, git_url):
        self.ui.status(_("fetching from git url: " + git_url + "\n"))
        self.export_git_objects()
        self.fetch_pack(git_url)
        self.import_git_objects()
        self.save_map()

    def fetch_pack(self, git_url):
        client, path = self.get_transport_and_path(git_url)
        graphwalker = SimpleFetchGraphWalker(self.git.heads().values(), self.git.get_parents)
        f, commit = self.git.object_store.add_pack()
        try:
            determine_wants = self.git.object_store.determine_wants_all
            refs = client.fetch_pack(path, determine_wants, graphwalker, f.write, sys.stdout.write)
            f.close()
            commit()
            self.git.set_refs(refs)
        except:
            f.close()
            raise    

    def import_git_objects(self):
        self.ui.status(_("importing Git objects into Hg\n"))
        # import heads as remote references
        todo = []
        done = set()
        convert_list = {}
        
        # get a list of all the head shas
        for head, sha in self.git.heads().iteritems():
            todo.append(sha)
        
        # traverse the heads getting a list of all the unique commits
        # TODO : stop when we hit a SHA we've already imported
        while todo:
            sha = todo.pop()
            assert isinstance(sha, str)
            if sha in done:
                continue
            done.add(sha)
            commit = self.git.commit(sha)
            convert_list[sha] = commit
            todo.extend([p for p in commit.parents if p not in done])
        
        # sort the commits by commit date (TODO: change to topo sort)
        commits = TopoSort(convert_list).items()
        
        # import each of the commits, oldest first
        for csha in commits:
            commit = convert_list[csha]
            self.import_git_commit(commit)
    
        # TODO : update Hg bookmarks (possibly named heads?)
        print bookmarks.parse(self.repo)

    def import_git_commit(self, commit):
        print "importing: " + commit.id
        
        # TODO : have to handle merge contexts at some point (two parent files, etc)
        def getfilectx(repo, memctx, f):
            (e, sha, data) = self.git.get_file(commit, f)
            e = '' # TODO : make this a real mode
            return context.memfilectx(f, data, 'l' in e, 'x' in e, None)
        
        p1 = "0" * 40
        p2 = "0" * 40
        # TODO : do something if parent is not mapped yet!
        if len(commit.parents) > 0:
            sha = commit.parents[0]
            p1 = self._map[sha]
        if len(commit.parents) > 1:
            sha = commit.parents[1]
            p2 = self._map[sha]
        if len(commit.parents) > 2:
            # TODO : map extra parents to the extras file
            pass

        files = self.git.get_files_changed(commit)
        #print files

        # get a list of the changed, added, removed files
        extra = {}
        text = commit.message
        date = datetime.datetime.fromtimestamp(commit.author_time).strftime("%Y-%m-%d %H:%M:%S")
        ctx = context.memctx(self.repo, (p1, p2), text, files, getfilectx,
                             commit.author, date, extra)
        a = self.repo.commitctx(ctx)

        # get changeset id
        p2 = hex(self.repo.changelog.tip())
        # save changeset to mapping file
        gitsha = commit.id
        self._map[gitsha] = p2
        
    def getfilectx(self, source, repo, memctx, f):
        v = files[f]
        data = source.getfile(f, v)
        e = source.getmode(f, v)
        return context.memfilectx(f, data, 'l' in e, 'x' in e, copies.get(f))

    def export_git_objects(self):
        pass

    def check_bookmarks(self):
        if self.ui.config('extensions', 'hgext.bookmarks') is not None:
            print "YOU NEED TO SETUP BOOKMARKS"

    def get_transport_and_path(self, uri):
        from dulwich.client import TCPGitClient, SSHGitClient, SubprocessGitClient
        for handler, transport in (("git://", TCPGitClient), ("git+ssh://", SSHGitClient)):
            if uri.startswith(handler):
                host, path = uri[len(handler):].split("/", 1)
                return transport(host), "/"+path
        # if its not git or git+ssh, try a local url..
        return SubprocessGitClient(), uri


"""
   Tarjan's algorithm and topological sorting implementation in Python
   by Paul Harrison
   Public domain, do with it as you will
"""
class TopoSort(object):
    
    def __init__(self, commitdict):
        self._sorted = self.robust_topological_sort(commitdict)
        self._shas = []
        for level in self._sorted:
            self._shas.append(level[0])
            
    def items(self):
        self._shas.reverse()
        return self._shas
        
    def strongly_connected_components(self, graph):
        """ Find the strongly connected components in a graph using
            Tarjan's algorithm.

            graph should be a dictionary mapping node names to
            lists of successor nodes.
            """

        result = [ ]
        stack = [ ]
        low = { }

        def visit(node):
            if node in low: return

            num = len(low)
            low[node] = num
            stack_pos = len(stack)
            stack.append(node)

            for successor in graph[node].parents:
                visit(successor)
                low[node] = min(low[node], low[successor])

            if num == low[node]:
                component = tuple(stack[stack_pos:])
                del stack[stack_pos:]
                result.append(component)
                for item in component:
                    low[item] = len(graph)

        for node in graph:
            visit(node)

        return result


    def topological_sort(self, graph):
        count = { }
        for node in graph:
            count[node] = 0
        for node in graph:
            for successor in graph[node]:
                count[successor] += 1

        ready = [ node for node in graph if count[node] == 0 ]

        result = [ ]
        while ready:
            node = ready.pop(-1)
            result.append(node)

            for successor in graph[node]:
                count[successor] -= 1
                if count[successor] == 0:
                    ready.append(successor)

        return result


    def robust_topological_sort(self, graph):
        """ First identify strongly connected components,
            then perform a topological sort on these components. """

        components = self.strongly_connected_components(graph)

        node_component = { }
        for component in components:
            for node in component:
                node_component[node] = component

        component_graph = { }
        for component in components:
            component_graph[component] = [ ]

        for node in graph:
            node_c = node_component[node]
            for successor in graph[node].parents:
                successor_c = node_component[successor]
                if node_c != successor_c:
                    component_graph[node_c].append(successor_c) 

        return self.topological_sort(component_graph)
