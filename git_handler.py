import os, errno, sys, time, datetime, pickle, copy
import dulwich
from dulwich.repo import Repo
from dulwich.client import SimpleFetchGraphWalker
from dulwich.objects import hex_to_sha
from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid
from mercurial import hg, util, context, error

import math

def seconds_to_offset(time):
    hours = (float(time) / 60 / 60)
    hour_diff = math.fmod(time, 60)
    minutes = int(hour_diff)
    hours = int(math.floor(hours))
    if hours > 12:
        sign = '+'
        hours = 12 - (hours - 12)
    else:
        sign = '-'
    return sign + str(hours).rjust(2, '0') + str(minutes).rjust(2, '0')

def offset_to_seconds(offset):
    if len(offset) == 5:
        sign = offset[0:1]
        hours = int(offset[1:3])
        minutes = int(offset[3:5])
        if sign == '+':
            hours = 12 + (12 - hours)
        return (hours * 60 * 60) + (minutes) * 60
    else:
        return 0

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
        self.update_references()
        self.upload_pack(remote_name)
        self.save_map()

    # TODO: make these actually save and recall
    def remote_add(self, remote_name, git_url):
        self._config['remote.' + remote_name + '.url'] = git_url
        self.save_config()

    def remote_name_to_url(self, remote_name):
        return self._config['remote.' + remote_name + '.url']

    def update_references(self):
        # TODO : if bookmarks exist, add them as git branches
        c = self.map_git_get(hex(self.repo.changelog.tip()))
        self.git.set_ref('refs/heads/master', c)
        
    def export_git_objects(self):
        print "exporting git objects"
        for rev in self.repo.changelog:
            self.export_hg_commit(rev)
                
    # convert this commit into git objects
    # go through the manifest, convert all blobs/trees we don't have
    # write the commit object (with metadata info)
    def export_hg_commit(self, rev):
        # return if we've already processed this
        node = self.repo.changelog.lookup(rev)        
        phgsha = hex(node)
        pgit_sha = self.map_git_get(phgsha)
        if pgit_sha:
            return pgit_sha
        
        print "converting revision " + str(rev)
        
        # make sure parents are converted first
        parents = self.repo.parents(rev)
        for parent in parents:
            p_rev = parent.rev()
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            if not git_sha:
                self.export_hg_commit(self, p_rev)
        
        ctx = self.repo.changectx(rev)
        tree_sha = self.write_git_tree(ctx)
        
        # TODO : something with tags?
        # TODO : explicit file renaming, copying?
        
        commit = {}
        commit['tree'] = tree_sha
        (time, timezone) = ctx.date()
        commit['author'] = ctx.user() + ' ' + str(int(time)) + ' ' + seconds_to_offset(timezone) 
        message = ctx.description()
        commit['message'] = ctx.description()
        commit['message'] += "\n\n--HG EXTRAS--\n"
        commit['message'] += "branch : " + ctx.branch() + "\n"
        
        commit['parents'] = []
        for parent in parents:
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            commit['parents'].append(git_sha)
            
        commit_sha = self.git.write_commit_hash(commit) # writing new blobs to git
        self.map_set(commit_sha, phgsha)
        return commit_sha
        
    def write_git_tree(self, ctx):
        trees = {}
        man = ctx.manifest()
        for filenm in man.keys():            
            # write blob if not in our git database
            fctx = ctx.filectx(filenm)
            is_exec = 'x' in fctx.flags()
            is_link = 'l' in fctx.flags()
            file_id = hex(fctx.filenode())
            blob_sha = self.map_git_get(file_id)
            if not blob_sha:
                blob_sha = self.git.write_blob(fctx.data()) # writing new blobs to git
                self.map_set(blob_sha, file_id)

            parts = filenm.split('/')
            if len(parts) > 1:

                # get filename and path for leading subdir
                filepath = parts[-1:][0]
                dirpath = "/".join([v for v in parts[0:-1]]) + '/'

                # get subdir name and path for parent dir
                parentsub = parts[-2:][0]
                parentpath = "/".join([v for v in parts[0:-2]]) + '/'

                # set file entry
                fileentry = ['blob', filepath, blob_sha, is_exec, is_link]
                if dirpath not in trees:
                    trees[dirpath] = []
                trees[dirpath].append(fileentry)

                # set directory entry
                treeentry = ['tree', parentsub + '/', dirpath]
                if parentpath not in trees:
                    trees[parentpath] = []
                if treeentry not in trees[parentpath]:
                    trees[parentpath].append( treeentry )
            else:
                fileentry = ['blob', parts[0], blob_sha, is_exec, is_link]                
                if '/' not in trees:
                    trees['/'] = []
                trees['/'].append(fileentry)

        # sort by tree depth, so we write the deepest trees first
        dirs = trees.keys()
        dirs.sort(lambda a, b: len(b.split('/'))-len(a.split('/')))

        # write all the trees
        tree_sha = None
        tree_shas = {}
        for dirnm in dirs:
            tree_data = []
            for entry in trees[dirnm]:
                # replace tree path with tree SHA
                if entry[0] == 'tree':
                    sha = tree_shas[entry[2]]
                    entry[2] = sha
                tree_data.append(entry)
            tree_sha = self.git.write_tree_array(tree_data) # writing new trees to git
            tree_shas[dirnm] = tree_sha
        return tree_sha # should be the last root tree sha
                
    def remote_head(self, remote_name):
        for head, sha in self.git.remote_refs(remote_name).iteritems():
            if head == 'HEAD':
                return self.map_hg_get(sha)
        return None

    def upload_pack(self, remote_name):
        git_url = self.remote_name_to_url(remote_name)
        client, path = self.get_transport_and_path(git_url)
        genpack = self.generate_pack_contents
        try:
            client.send_pack(path, genpack)
            # TODO : self.git.set_remote_refs(refs, remote_name)
        except:
            raise
            
    def generate_pack_contents(self, want, have, none):
        print "WANT: " + str(want)
        print "HAVE: " + str(have)
        print "NONE: " + str(none)
        
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
            try:
                commit = self.git.commit(sha)
                convert_list[sha] = commit
                todo.extend([p for p in commit.parents if p not in done])
            except:
                print "Cannot import tags yet" # TODO

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
        # TODO : look for HG metadata in the message and use it
        
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
        for handler, transport in (("git://", TCPGitClient), ("git@", SSHGitClient)):
            if uri.startswith(handler):
                if handler == 'git@':
                    host, path = uri[len(handler):].split(":", 1)
                    host = 'git@' + host
                else:
                    host, path = uri[len(handler):].split('/', 1)
                return transport(host), '/' + path
        # if its not git or git+ssh, try a local url..
        return SubprocessGitClient(), uri

''
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
