import os, errno, sys, time, datetime, pickle, copy
import dulwich
from dulwich.repo import Repo
from dulwich.client import SimpleFetchGraphWalker
from dulwich.objects import hex_to_sha
from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid
from mercurial import hg, util, context, error

class GitHandler(object):

    def __init__(self, dest_repo, ui):
        self.repo = dest_repo
        self.ui = ui
        self.init_if_missing()
        self.load_git()
        self.load_map()
        self.load_config()

    # make the git data directory
    def init_if_missing(self):
        git_hg_path = os.path.join(self.repo.path, 'git')
        if not os.path.exists(git_hg_path):
            os.mkdir(git_hg_path)
            dulwich.repo.Repo.init_bare(git_hg_path)

    def load_git(self):
        git_dir = os.path.join(self.repo.path, 'git')
        self.git = Repo(git_dir)

    ## FILE LOAD AND SAVE METHODS

    def map_set(self, gitsha, hgsha):
        self._map_git[gitsha] = hgsha
        self._map_hg[hgsha] = gitsha

    def map_hg_get(self, gitsha):
        if gitsha in self._map_git:
            return self._map_git[gitsha]
        else:
            return None

    def map_git_get(self, hgsha):
        if hgsha in self._map_hg:
            return self._map_hg[hgsha]
        else:
            return None
        
    def load_map(self):
        self._map_git = {}
        self._map_hg = {}
        if os.path.exists(self.repo.join('git-mapfile')):
            for line in self.repo.opener('git-mapfile'):
                gitsha, hgsha = line.strip().split(' ', 1)
                self._map_git[gitsha] = hgsha
                self._map_hg[hgsha] = gitsha

    def save_map(self):
        file = self.repo.opener('git-mapfile', 'w+')
        for gitsha, hgsha in self._map_git.iteritems():
            file.write("%s %s\n" % (gitsha, hgsha))
        file.close()

    def load_config(self):
        self._config = {}
        if os.path.exists(self.repo.join('git-config')):
            for line in self.repo.opener('git-config'):
                key, value = line.strip().split(' ', 1)
                self._config[key] = value

    def save_config(self):
        file = self.repo.opener('git-config', 'w+')
        for key, value in self._config.iteritems():
            file.write("%s %s\n" % (key, value))
        file.close()


    ## END FILE LOAD AND SAVE METHODS

    def fetch(self, remote_name):
        self.ui.status(_("fetching from : " + remote_name + "\n"))
        self.export_git_objects()
        self.fetch_pack(remote_name)
        self.import_git_objects(remote_name)
        self.save_map()

    def push(self, remote_name):
        self.ui.status(_("pushing to : " + remote_name + "\n"))
        self.export_git_objects()
        self.upload_pack(remote_name)
        self.save_map()

    # TODO: make these actually save and recall
    def remote_add(self, remote_name, git_url):
        self._config['remote.' + remote_name + '.url'] = git_url
        self.save_config()

    def remote_name_to_url(self, remote_name):
        return self._config['remote.' + remote_name + '.url']

    def export_git_objects(self):
        print "exporting git objects"
        for rev in self.repo.changelog:
            node = self.repo.changelog.lookup(rev)
            hgsha = hex(node)
            git_sha = self.map_git_get(hgsha)
            if not git_sha:
                self.export_hg_commit(rev)
                
    def export_hg_commit(self, rev):
        # convert this commit into git objects
        # go through the manifest, convert all blobs/trees we don't have
        # write the commit object (with metadata info)
        print "EXPORT"
        node = self.repo.changelog.lookup(rev)
        parents = self.repo.parents(rev)
        
        print parents # TODO: make sure parents are converted first
        
        ctx = self.repo.changectx(rev)
        man = ctx.manifest()
        for filename in man.keys():
            print "WRITE BLOB/TREE OBJECT"
            print filename
            fctx = ctx.filectx(filename)
            file_id = hex(fctx.filenode())
            git_sha = self.map_git_get(file_id)
            print git_sha
            
            print fctx.data()
            self.git.write_blob(fctx.data())

        print "WRITE COMMIT OBJECT"
        print ctx.user()
        print ctx.date()
        print ctx.description()
        print ctx.branch()
        print ctx.tags()
            
            
    def remote_head(self, remote_name):
        for head, sha in self.git.remote_refs(remote_name).iteritems():
            if head == 'HEAD':
                return self.map_hg_get(sha)
        return None

    def upload_pack(self, remote_name):
        self.ui.status(_("upload pack\n"))

    def fetch_pack(self, remote_name):
        git_url = self.remote_name_to_url(remote_name)
        client, path = self.get_transport_and_path(git_url)
        graphwalker = SimpleFetchGraphWalker(self.git.heads().values(), self.git.get_parents)
        f, commit = self.git.object_store.add_pack()
        try:
            determine_wants = self.git.object_store.determine_wants_all
            refs = client.fetch_pack(path, determine_wants, graphwalker, f.write, sys.stdout.write)
            f.close()
            commit()
            self.git.set_remote_refs(refs, remote_name)
        except:
            f.close()
            raise

    def import_git_objects(self, remote_name):
        self.ui.status(_("importing Git objects into Hg\n"))
        # import heads as remote references
        todo = []
        done = set()
        convert_list = {}

        # get a list of all the head shas
        for head, sha in self.git.remote_refs(remote_name).iteritems():
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

        # sort the commits
        commits = TopoSort(convert_list).items()

        # import each of the commits, oldest first
        for csha in commits:
            commit = convert_list[csha]
            self.import_git_commit(commit)

        # update Hg bookmarks
        bms = {}
        for head, sha in self.git.remote_refs(remote_name).iteritems():
            hgsha = hex_to_sha(self.map_hg_get(sha))
            if not head == 'HEAD':
                bms[remote_name + '/' + head] = hgsha
        bookmarks.write(self.repo, bms)

    def import_git_commit(self, commit):
        print "importing: " + commit.id

        # TODO : (?) have to handle merge contexts at some point (two parent files, etc)
        def getfilectx(repo, memctx, f):
            (e, sha, data) = self.git.get_file(commit, f)
            e = '' # TODO : make this a real mode
            return context.memfilectx(f, data, 'l' in e, 'x' in e, None)

        p1 = "0" * 40
        p2 = "0" * 40
        if len(commit.parents) > 0:
            sha = commit.parents[0]
            p1 = self.map_hg_get(sha)
        if len(commit.parents) > 1:
            sha = commit.parents[1]
            p2 = self.map_hg_get(sha)
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
        self.map_set(gitsha, p2)

    def getfilectx(self, source, repo, memctx, f):
        v = files[f]
        data = source.getfile(f, v)
        e = source.getmode(f, v)
        return context.memfilectx(f, data, 'l' in e, 'x' in e, copies.get(f))

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
            for sha in level:
                self._shas.append(sha)

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
