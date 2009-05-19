import os, errno, sys, time, datetime, pickle, copy, math
import toposort
import dulwich
from dulwich.repo import Repo
from dulwich.client import SimpleFetchGraphWalker
from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid
from mercurial import hg, util, context, error
from dulwich.objects import (
    Blob,
    Commit,
    ShaFile,
    Tag,
    Tree,
    hex_to_sha,
    format_timezone,
    )

import math

def seconds_to_offset(time):
    hours = (float(time) / 60 / 60)
    hour_diff = math.fmod(time, 60)
    minutes = int(hour_diff)
    hours = int(math.floor(hours))
    if hours > 12:
        sign = '+'
        hours = 12 - (hours - 12)
    elif hours > 0:
        sign = '-'
    else:
        sign = ''
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
        self.mapfile = 'git-mapfile'
        self.configfile = 'git-config'
        self.gitdir = self.repo.join('git')
        self.init_if_missing()
        self.load_git()
        self.load_map()
        self.load_config()

    # make the git data directory
    def init_if_missing(self):
        if not os.path.exists(self.gitdir):
            os.mkdir(self.gitdir)
            Repo.init_bare(self.gitdir)

    def load_git(self):
        self.git = Repo(self.gitdir)

    ## FILE LOAD AND SAVE METHODS

    def map_set(self, gitsha, hgsha):
        self._map_git[gitsha] = hgsha
        self._map_hg[hgsha] = gitsha

    def map_hg_get(self, gitsha):
	return self._map_git.get(gitsha)

    def map_git_get(self, hgsha):
	return self._map_hg.get(hgsha)

    def load_map(self):
        self._map_git = {}
        self._map_hg = {}
        if os.path.exists(self.repo.join(self.mapfile)):
            for line in self.repo.opener(self.mapfile):
                gitsha, hgsha = line.strip().split(' ', 1)
                self._map_git[gitsha] = hgsha
                self._map_hg[hgsha] = gitsha

    def save_map(self):
        file = self.repo.opener(self.mapfile, 'w+', atomictemp=True)
        for gitsha, hgsha in sorted(self._map_git.iteritems()):
            file.write("%s %s\n" % (gitsha, hgsha))
        file.rename()

    def load_config(self):
        self._config = {}
        if os.path.exists(self.repo.join(self.configfile)):
            for line in self.repo.opener(self.configfile):
                key, value = line.strip().split(' ', 1)
                self._config[key] = value

    def save_config(self):
        file = self.repo.opener(self.configfile, 'w+', atomictemp=True)
        for key, value in self._config.iteritems():
            file.write("%s %s\n" % (key, value))
        file.rename()


    ## END FILE LOAD AND SAVE METHODS

    def fetch(self, remote_name):
        self.ui.status(_("fetching from : %s\n") % remote_name)
        self.export_git_objects()
        refs = self.fetch_pack(remote_name)
        if refs:
            self.import_git_objects(remote_name)
        self.save_map()

    def export(self):
        self.export_git_objects()
        self.update_references()
        self.save_map()

    def push(self, remote_name):
        self.ui.status(_("pushing to : %s\n") % remote_name)
        self.export()
        self.update_remote_references(remote_name)
        self.upload_pack(remote_name)

    def remote_add(self, remote_name, git_url):
        self._config['remote.' + remote_name + '.url'] = git_url
        self.save_config()

    def remote_remove(self, remote_name):
        key = 'remote.' + remote_name + '.url'
        if key in self._config:
            del self._config[key]
        self.save_config()

    def remote_show(self, remote_name):
        key = 'remote.' + remote_name + '.url'
        if key in self._config:
            name = self._config[key]
            self.ui.status(_("URL for %s : %s\n") % (remote_name, name, ))
        else:
            self.ui.status(_("No remote named : %s\n") % remote_name)
        return

    def remote_list(self):
        for key, value in self._config.iteritems():
            if key[0:6] == 'remote':
                self.ui.status('%s\t%s\n' % (key, value, ))

    def remote_name_to_url(self, remote_name):
        return self._config['remote.' + remote_name + '.url']

    def update_references(self):
        try:
            # We only care about bookmarks of the form 'name',
            # not 'remote/name'.
            def is_local_ref(item): return item[0].count('/') == 0
            bms = bookmarks.parse(self.repo)
            bms = dict(filter(is_local_ref, bms.items()))

            # Create a local Git branch name for each
            # Mercurial bookmark.
            for key in bms:
                hg_sha  = hex(bms[key])
                git_sha = self.map_git_get(hg_sha)
                self.git.set_ref('refs/heads/' + key, git_sha)
        except AttributeError:
            # No bookmarks extension
            pass

        c = self.map_git_get(hex(self.repo.changelog.tip()))
        self.git.set_ref('refs/heads/master', c)

    # Make sure there's a refs/remotes/remote_name/name
    #           for every refs/heads/name
    def update_remote_references(self, remote_name):
        self.git.set_remote_refs(self.local_heads(), remote_name)

    def local_heads(self):
        def is_local_head(item): return item[0].startswith('refs/heads')
        refs = self.git.get_refs()
        return dict(filter(is_local_head, refs.items()))

    def export_git_objects(self):
        self.ui.status(_("exporting git objects\n"))
        total = len(self.repo.changelog)
        magnitude = int(math.log(total, 10)) + 1 if total else 1
        for i, rev in enumerate(self.repo.changelog):
            if i%100 == 0:
                self.ui.status(_("at: %*d/%d\n") % (magnitude, i, total))
            pgit_sha, already_written = self.export_hg_commit(rev)
            if not already_written:
                self.save_map()

    # convert this commit into git objects
    # go through the manifest, convert all blobs/trees we don't have
    # write the commit object (with metadata info)
    def export_hg_commit(self, rev):
        # return if we've already processed this
        node = self.repo.changelog.lookup(rev)
        phgsha = hex(node)
        pgit_sha = self.map_git_get(phgsha)
        if pgit_sha:
            return pgit_sha, True

        self.ui.status(_("converting revision %s\n") % str(rev))

        # make sure parents are converted first
        parents = self.repo.parents(rev)
        for parent in parents:
            p_rev = parent.rev()
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            if not p_rev == -1:
                if not git_sha:
                    self.export_hg_commit(p_rev)

        ctx = self.repo.changectx(rev)
        tree_sha, renames = self.write_git_tree(ctx)
        
        commit = {}
        commit['tree'] = tree_sha
        (time, timezone) = ctx.date()

        # hg authors might not have emails
        author = ctx.user()
        if not '>' in author: # TODO : this kills losslessness - die (submodules)?
            author = author + ' <none@none>'
        commit['author'] = author + ' ' + str(int(time)) + ' ' + format_timezone(-timezone)
        message = ctx.description()
        commit['message'] = ctx.description() + "\n"

        extra = ctx.extra()
        if 'committer' in extra:
            # fixup timezone
            (name_timestamp, timezone) = extra['committer'].rsplit(' ', 1)
            timezone = format_timezone(-int(timezone))
            commit['committer'] = '%s %s' % (name_timestamp, timezone)
        if 'encoding' in extra:
            commit['encoding'] = extra['encoding']

        # HG EXTRA INFORMATION
        add_extras = False
        extra_message = ''
        if not ctx.branch() == 'default':
            add_extras = True
            extra_message += "branch : " + ctx.branch() + "\n"

        if renames:
            add_extras = True
            for oldfile, newfile in renames:
                extra_message += "rename : " + oldfile + " => " + newfile + "\n"
            
        if add_extras:
            commit['message'] += "\n--HG--\n" + extra_message

        commit['parents'] = []
        for parent in parents:
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            if git_sha:
                commit['parents'].append(git_sha)

        commit_sha = self.git.write_commit_hash(commit) # writing new blobs to git
        self.map_set(commit_sha, phgsha)
        return commit_sha, False

    def write_git_tree(self, ctx):
        trees = {}
        man = ctx.manifest()
        renames = []
        for filenm in man.keys():
            # write blob if not in our git database
            fctx = ctx.filectx(filenm)
            rename = fctx.renamed()
            if rename:
                filerename, sha = rename
                renames.append((filerename, filenm))
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
                parpath = '/'
                nparpath = '/'
                for part in parts[0:-1]:
                    if nparpath == '/':
                        nparpath = part + '/'
                    else:
                        nparpath += part + '/'

                    treeentry = ['tree', part + '/', nparpath]

                    if parpath not in trees:
                        trees[parpath] = []
                    if treeentry not in trees[parpath]:
                        trees[parpath].append( treeentry )

                    parpath = nparpath

                # set file entry
                fileentry = ['blob', filepath, blob_sha, is_exec, is_link]
                if dirpath not in trees:
                    trees[dirpath] = []
                trees[dirpath].append(fileentry)

            else:
                fileentry = ['blob', parts[0], blob_sha, is_exec, is_link]
                if '/' not in trees:
                    trees['/'] = []
                trees['/'].append(fileentry)

        dirs = trees.keys()
        if dirs:
            # sort by tree depth, so we write the deepest trees first
            dirs.sort(lambda a, b: len(b.split('/'))-len(a.split('/')))
            dirs.remove('/')
            dirs.append('/')
        else:
            # manifest is empty => make empty root tree
            trees['/'] = []
            dirs = ['/']

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
        
        return (tree_sha, renames) # should be the last root tree sha

    def remote_head(self, remote_name):
        for head, sha in self.git.remote_refs(remote_name).iteritems():
            if head == 'HEAD':
                return self.map_hg_get(sha)
        return None

    def upload_pack(self, remote_name):
        git_url = self.remote_name_to_url(remote_name)
        client, path = self.get_transport_and_path(git_url)
        changed = self.get_changed_refs
        genpack = self.generate_pack_contents
        try:
            self.ui.status(_("creating and sending data\n"))
            changed_refs = client.send_pack(path, changed, genpack)
            if changed_refs:
                new_refs = {}
                for ref, sha in changed_refs.iteritems():
                    self.ui.status("    "+ remote_name + "::" + ref + " => GIT:" + sha[0:8] + "\n")
                    new_refs[ref] = sha
                self.git.set_remote_refs(new_refs, remote_name)
                self.update_hg_bookmarks(remote_name)
        except:
            # TODO : remove try/except or do something useful here
            raise

    # TODO : for now, we'll just push all heads that match remote heads
    #        * we should have specified push, tracking branches and --all
    # takes a dict of refs:shas from the server and returns what should be
    # pushed up
    def get_changed_refs(self, refs):
        keys = refs.keys()

        changed = {}
        if not keys:
            return None

        # TODO : this is a huge hack
        if keys[0] == 'capabilities^{}': # nothing on the server yet - first push
            changed['refs/heads/master'] = self.git.ref('master')

        for ref_name in keys:
            parts = ref_name.split('/')
            if parts[0] == 'refs': # strip off 'refs/heads'
                if parts[1] == 'heads':
                    head = "/".join([v for v in parts[2:]])
                    local_ref = self.git.ref(ref_name)
                    if local_ref:
                        if not local_ref == refs[ref_name]:
                            changed[ref_name] = local_ref
        
        # Also push any local branches not on the server yet
        for head in self.local_heads():
            if not head in refs:
                ref = self.git.ref(head)
                changed[head] = ref

        return changed

    # takes a list of shas the server wants and shas the server has
    # and generates a list of commit shas we need to push up
    def generate_pack_contents(self, want, have):
        graph_walker = SimpleFetchGraphWalker(want, self.git.get_parents)
        next = graph_walker.next()
        shas = set()
        while next:
            if next in have:
                graph_walker.ack(next)
            else:
                shas.add(next)
            next = graph_walker.next()
        
        seen = []
        
        # so now i have the shas, need to turn them into a list of
        # tuples (sha, path) for ALL the objects i'm sending
        # TODO : don't send blobs or trees they already have
        def get_objects(tree, path):
            changes = list()
            changes.append((tree, path))
            for (mode, name, sha) in tree.entries():
                if mode == 0160000: # TODO : properly handle submodules and document what 57344 means
                    continue
                if sha in seen:
                    continue
                    
                obj = self.git.get_object(sha)
                seen.append(sha)
                if isinstance (obj, Blob):
                    changes.append((obj, path + name))
                elif isinstance(obj, Tree):
                    changes.extend(get_objects(obj, path + name + '/'))
            return changes

        objects = []
        for commit_sha in shas:
            commit = self.git.commit(commit_sha)
            objects.append((commit, 'commit'))
            tree = self.git.get_object(commit.tree)
            objects.extend( get_objects(tree, '/') )

        return objects

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
            if refs:
                self.git.set_remote_refs(refs, remote_name)
            else:
                self.ui.status(_("nothing new on the server\n"))
            return refs
        except:
            f.close()
            raise

    def import_git_objects(self, remote_name):
        self.ui.status(_("importing Git objects into Hg\n"))
        # import heads as remote references
        todo = []
        done = set()
        convert_list = {}
        self.renames = {}
        
        # get a list of all the head shas
        for head, sha in self.git.remote_refs(remote_name).iteritems():
            todo.append(sha)

        # traverse the heads getting a list of all the unique commits
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
                self.ui.warn(_("Cannot import tags yet\n")) # TODO

        # sort the commits
        commits = toposort.TopoSort(convert_list).items()
        
        # import each of the commits, oldest first
        for csha in commits:
            commit = convert_list[csha]
            if not self.map_hg_get(csha): # it's already here
                self.import_git_commit(commit)
            else:
                self.pseudo_import_git_commit(commit)
                
        self.update_hg_bookmarks(remote_name)

    def update_hg_bookmarks(self, remote_name):
        try:
            bms = bookmarks.parse(self.repo)
            for head, sha in self.git.remote_refs(remote_name).iteritems():
                hgsha = hex_to_sha(self.map_hg_get(sha))
                if not head == 'HEAD':
                    bms[remote_name + '/' + head] = hgsha
            bookmarks.write(self.repo, bms)
        except AttributeError:
            self.ui.warn(_('creating bookmarks failed, do you have'
                         ' bookmarks enabled?\n'))

    def convert_git_int_mode(self, mode):
	# TODO : make these into constants
        convert = {
         0100644: '',
         0100755: 'x',
         0120000: 'l'}
        if mode in convert:
            return convert[mode]
        return ''

    def extract_hg_metadata(self, message):
        split = message.split("\n\n--HG--\n", 1)
        renames = {}
        branch = False
        if len(split) == 2:
            message, meta = split
            lines = meta.split("\n")
            for line in lines:
                if line == '':
                    continue 
                
                command, data = line.split(" : ", 1)
                if command == 'rename':
                    before, after = data.split(" => ", 1)
                    renames[after] = before
                if command == 'branch':
                    branch = data
        return (message, renames, branch)

    def pseudo_import_git_commit(self, commit):
        (strip_message, hg_renames, hg_branch) = self.extract_hg_metadata(commit.message)
        cs = self.map_hg_get(commit.id)
        p1 = nullid
        p2 = nullid
        if len(commit.parents) > 0:
            sha = commit.parents[0]
            p1 = self.map_hg_get(sha)
        if len(commit.parents) > 1:
            sha = commit.parents[1]
            p2 = self.map_hg_get(sha)
        if len(commit.parents) > 2:
            # TODO : map extra parents to the extras file
            pass
        # saving rename info
        if (not (p2 == nullid) or (p1 == nullid)):
            self.renames[cs] = {}
        else:
            self.renames[cs] = self.renames[p1].copy()

        self.renames[cs].update(hg_renames)
    
    def import_git_commit(self, commit):
        self.ui.debug(_("importing: %s\n") % commit.id)
        # TODO : find and use hg named branches
        # TODO : add extra Git data (committer info) as extras to changeset

        # TODO : (?) have to handle merge contexts at some point (two parent files, etc)
        # TODO : Do something less coarse-grained than try/except on the
        #        get_file call for removed files
        
        (strip_message, hg_renames, hg_branch) = self.extract_hg_metadata(commit.message)
        
        def getfilectx(repo, memctx, f):
            try:
                (mode, sha, data) = self.git.get_file(commit, f)
                e = self.convert_git_int_mode(mode)
            except TypeError:
                raise IOError()
            if f in hg_renames:
                copied_path = hg_renames[f]
            else:
                copied_path = None
            return context.memfilectx(f, data, 'l' in e, 'x' in e, copied_path)

        p1 = nullid
        p2 = nullid
        if len(commit.parents) > 0:
            sha = commit.parents[0]
            p1 = self.map_hg_get(sha)
        if len(commit.parents) > 1:
            sha = commit.parents[1]
            p2 = self.map_hg_get(sha)
        if len(commit.parents) > 2:
            # TODO : map extra parents to the extras file
            pass

        # get a list of the changed, added, removed files
        files = self.git.get_files_changed(commit)

        # wierd hack for explicit file renames in first but not second branch
        if not (p2 == nullid):
            vals = [item for item in self.renames[p1].values() if not item in self.renames[p2].values()]
            for removefile in vals:
                files.remove(removefile)
            
        extra = {}

        # if named branch, add to extra
        if hg_branch:
            extra['branch'] = hg_branch

        # if committer is different than author, add it to extra
        if not commit._author_raw == commit._committer_raw:
            extra['committer'] = "%s %d %d" % (commit.committer, commit.commit_time, -commit.commit_timezone)

        if commit._encoding:
            extra['encoding'] = commit._encoding

        if hg_branch:
            extra['branch'] = hg_branch

        text = strip_message
        date = (commit.author_time, -commit.author_timezone)
        ctx = context.memctx(self.repo, (p1, p2), text, files, getfilectx,
                             commit.author, date, extra)
        a = self.repo.commitctx(ctx)

        # get changeset id
        cs = hex(self.repo.changelog.tip())
        # save changeset to mapping file
        gitsha = commit.id
        
        # saving rename info
        if (not (p2 == nullid) or (p1 == nullid)):
            self.renames[cs] = {}
        else:
            self.renames[cs] = self.renames[p1].copy()
            
        self.renames[cs].update(hg_renames)
        
        self.map_set(gitsha, cs)

    def check_bookmarks(self):
        if self.ui.config('extensions', 'hgext.bookmarks') is not None:
            self.ui.warn("YOU NEED TO SETUP BOOKMARKS\n")

    def get_transport_and_path(self, uri):
        from dulwich.client import TCPGitClient, SSHGitClient, SubprocessGitClient
        for handler, transport in (("git://", TCPGitClient), ("git@", SSHGitClient), ("git+ssh://", SSHGitClient)):
            if uri.startswith(handler):
                if handler == 'git@':
                    host, path = uri[len(handler):].split(":", 1)
                    host = 'git@' + host
                else:
                    host, path = uri[len(handler):].split("/", 1)
                return transport(host), '/' + path
        # if its not git or git+ssh, try a local url..
        return SubprocessGitClient(), uri

    def clear(self):
        mapfile = self.repo.join(self.mapfile)
        if os.path.exists(self.gitdir):
            for root, dirs, files in os.walk(self.gitdir, topdown=False):
                for name in files:
                    os.remove(os.path.join(root, name))
                for name in dirs:
                    os.rmdir(os.path.join(root, name))
            os.rmdir(self.gitdir)
        if os.path.exists(mapfile):
            os.remove(mapfile)
