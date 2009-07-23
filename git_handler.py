import os, sys, math, urllib, re
import toposort
from dulwich.repo import Repo
from dulwich.client import SimpleFetchGraphWalker
from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import hex, bin, nullid
from mercurial import context
from mercurial.error import RepoError
from dulwich.misc import make_sha
from dulwich.objects import (
    Blob,
    Commit,
    Tag,
    Tree,
    format_timezone,
    )


class GitHandler(object):

    def __init__(self, dest_repo, ui):
        self.repo = dest_repo
        self.ui = ui
        self.mapfile = 'git-mapfile'
        self.tagsfile = 'git-tags'

        if ui.config('git', 'intree'):
            self.gitdir = self.repo.wjoin('.git')
        else:
            self.gitdir = self.repo.join('git')

        self.paths = ui.configitems('paths')

        self.init_if_missing()
        self.load_git()
        self.load_map()
        self.load_tags()

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


    def load_tags(self):
        self.tags = {}
        if os.path.exists(self.repo.join(self.tagsfile)):
            for line in self.repo.opener(self.tagsfile):
                sha, name = line.strip().split(' ', 1)
                self.tags[name] = sha

    def save_tags(self):
        file = self.repo.opener(self.tagsfile, 'w+', atomictemp=True)
        for name, sha in sorted(self.tags.iteritems()):
            if not self.repo.tagtype(name) == 'global':
                file.write("%s %s\n" % (sha, name))
        file.rename()

    ## END FILE LOAD AND SAVE METHODS

    ## COMMANDS METHODS

    def import_commits(self, remote_name):
        self.import_git_objects(remote_name)
        self.save_map()

    def fetch(self, remote):
        self.export_git_objects()
        refs = self.fetch_pack(remote)
        remote_name = self.remote_name(remote)

        if refs:
            self.import_git_objects(remote_name, refs)
            self.import_tags(refs)
            self.update_hg_bookmarks(refs)
            if remote_name:
                self.update_remote_branches(remote_name, refs)
            elif not self.paths:
                # intial cloning
                self.update_remote_branches('default', refs)
        else:
            self.ui.status(_("nothing new on the server\n"))

        self.save_map()

    def export_commits(self, export_objects=True):
        if export_objects:
            self.export_git_objects()
        self.export_hg_tags()
        self.update_references()
        self.save_map()

    def push(self, remote):
        self.export_commits()
        changed_refs = self.upload_pack(remote)
        remote_name = self.remote_name(remote)

        if remote_name and changed_refs:
            for ref, sha in changed_refs.iteritems():
                self.ui.status("    "+ remote_name + "::" + ref + " => GIT:" + sha[0:8] + "\n")

            self.update_remote_branches(remote_name, changed_refs)


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

    ## CHANGESET CONVERSION METHODS

    def export_git_objects(self):
        self.previous_entries = {}
        self.written_trees = {}
        self.ui.status(_("importing Hg objects into Git\n"))
        nodes = [self.repo.lookup(n) for n in self.repo]
        export = [node for node in nodes if not hex(node) in self._map_hg]
        total = len(export)
        if total:
          magnitude = int(math.log(total, 10)) + 1
        else:
          magnitude = 1
        for i, rev in enumerate(export):
            if i%100 == 0:
                self.ui.status(_("at: %*d/%d\n") % (magnitude, i, total))

            ctx = self.repo.changectx(rev)
            state = ctx.extra().get('hg-git', None)
            if state == 'octopus':
                self.ui.debug("revision %d is a part of octopus explosion\n" % ctx.rev())
                continue
            self.export_hg_commit(rev)
            self.save_map()

    # convert this commit into git objects
    # go through the manifest, convert all blobs/trees we don't have
    # write the commit object (with metadata info)
    def export_hg_commit(self, rev):
        def is_octopus_part(ctx):
            return ctx.extra().get('hg-git', None) in set(['octopus', 'octopus-done'])

        self.ui.note(_("converting revision %s\n") % rev)

        oldenc = self.swap_out_encoding()

        # make sure parents are converted first
        ctx = self.repo.changectx(rev)
        extra = ctx.extra()

        parents = []
        if extra.get('hg-git', None) == 'octopus-done':
            # implode octopus parents
            part = ctx
            while is_octopus_part(part):
                (p1, p2) = part.parents()
                assert not is_octopus_part(p1)
                parents.append(p1)
                part = p2
            parents.append(p2)
        else:
            parents = ctx.parents()

        for parent in parents:
            p_node = parent.node()
            if p_node != nullid and not hex(p_node) in self._map_hg:
                self.export_hg_commit(p_rev)

        tree_sha, renames = self.write_git_tree(ctx)

        commit = {}
        commit['tree'] = tree_sha
        (time, timezone) = ctx.date()

        if 'author' in extra:
            author = extra['author']
        else:
            # hg authors might not have emails
            author = ctx.user()

            # check for git author pattern compliance
            regex = re.compile('^(.*?) \<(.*?)\>(.*)$')
            a = regex.match(author)

            if a:
                name = a.group(1)
                email = a.group(2)
                if len(a.group(3)) > 0:
                    name += ' ext:(' + urllib.quote(a.group(3)) + ')'
                author = name + ' <' + email + '>'
            else:
                author = author + ' <none@none>'

        commit['author'] = author + ' ' + str(int(time)) + ' ' + format_timezone(-timezone)

        if 'message' in extra:
            commit['message'] = extra['message']
        else:
            message = ctx.description()
            commit['message'] = ctx.description() + "\n"

        if 'committer' in extra:
            # fixup timezone
            (name_timestamp, timezone) = extra['committer'].rsplit(' ', 1)
            try:
                timezone = format_timezone(-int(timezone))
                commit['committer'] = '%s %s' % (name_timestamp, timezone)
            except ValueError: #pragma: no cover
                self.ui.warn(_("Ignoring committer in extra, invalid timezone in r%d: '%s'.\n") % (ctx, timezone))
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

        for key, value in extra.iteritems():
            if key in ('author', 'committer', 'encoding', 'message', 'branch', 'hg-git'):
                continue
            else:
                add_extras = True
                extra_message += "extra : " + key + " : " +  urllib.quote(value) + "\n"

        if add_extras:
            commit['message'] += "\n--HG--\n" + extra_message

        commit['parents'] = []
        for parent in parents:
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            if git_sha:
                commit['parents'].append(git_sha)

        commit_sha = self.git.write_commit_hash(commit) # writing new blobs to git
        self.map_set(commit_sha, ctx.hex())

        self.swap_out_encoding(oldenc)

        return commit_sha

    def write_git_tree(self, ctx):
        trees = {}
        man = ctx.manifest()
        ctx_id = hex(ctx.node())

        renames = []
        for filenm, nodesha in man.iteritems():
            file_id = hex(nodesha)
            if ctx_id not in self.previous_entries:
                self.previous_entries[ctx_id] = {}
            self.previous_entries[ctx_id][filenm] = file_id

            # write blob if not in our git database
            fctx = ctx.filectx(filenm)

            same_as_last = False
            for par in ctx.parents():
                par_id = hex(par.node())
                if par_id in self.previous_entries:
                    if filenm in self.previous_entries[par_id]:
                        if self.previous_entries[par_id][filenm] == file_id:
                            same_as_last = True
            if not same_as_last:
                rename = fctx.renamed()
                if rename:
                    filerename, sha = rename
                    renames.append((filerename, filenm))
            is_exec = 'x' in fctx.flags()
            is_link = 'l' in fctx.flags()
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
            sha_group = []

            # calculating a sha for the tree, so we don't write it twice
            listsha = make_sha()
            for entry in trees[dirnm]:
                # replace tree path with tree SHA
                if entry[0] == 'tree':
                    sha = tree_shas[entry[2]]
                    entry[2] = sha
                listsha.update(entry[1])
                listsha.update(entry[2])
                tree_data.append(entry)
            listsha = listsha.hexdigest()

            if listsha in self.written_trees:
                tree_sha = self.written_trees[listsha]
                tree_shas[dirnm] = tree_sha
            else:
                tree_sha = self.git.write_tree_array(tree_data) # writing new trees to git
                tree_shas[dirnm] = tree_sha
                self.written_trees[listsha] = tree_sha

        return (tree_sha, renames) # should be the last root tree sha

    def import_git_objects(self, remote_name=None, refs=None):
        self.ui.status(_("importing Git objects into Hg\n"))
        # import heads and fetched tags as remote references
        todo = []
        done = set()
        convert_list = {}

        # get a list of all the head shas
        if refs:
          for head, sha in refs.iteritems():
              todo.append(sha)
        elif remote_name:
            todo = self.git.remote_refs(remote_name).values()[:]
        else:
            todo = self.git.heads().values()[:]

        # traverse the heads getting a list of all the unique commits
        while todo:
            sha = todo.pop()
            assert isinstance(sha, str)
            if sha in done:
                continue
            done.add(sha)
            obj = self.git.get_object(sha)
            if isinstance (obj, Commit):
                convert_list[sha] = obj
                todo.extend([p for p in obj.parents if p not in done])
            if isinstance(obj, Tag):
                (obj_type, obj_sha) = obj.get_object()
                obj = self.git.get_object(obj_sha)
                if isinstance (obj, Commit):
                    convert_list[sha] = obj
                    todo.extend([p for p in obj.parents if p not in done])

        # sort the commits
        commits = toposort.TopoSort(convert_list).items()

        commits = [commit for commit in commits if not commit in self._map_git]
        # import each of the commits, oldest first
        total = len(commits)
        if total:
            magnitude = int(math.log(total, 10)) + 1
        else:
            magnitude = 1
        for i, csha in enumerate(commits):
            if i%100 == 0:
                self.ui.status(_("at: %*d/%d\n") % (magnitude, i, total))
            commit = convert_list[csha]
            self.import_git_commit(commit)

    def import_git_commit(self, commit):
        self.ui.debug(_("importing: %s\n") % commit.id)
        # TODO: Do something less coarse-grained than try/except on the
        #        get_file call for removed files

        (strip_message, hg_renames, hg_branch, extra) = self.extract_hg_metadata(commit.message)

        # get a list of the changed, added, removed files
        files = self.git.get_files_changed(commit)

        date = (commit.author_time, -commit.author_timezone)
        text = strip_message

        try:
            text.decode('utf-8')
        except UnicodeDecodeError:
            extra['message'] = text
            text = self.decode_guess(text, commit._encoding)

        author = commit.author

        # convert extra data back to the end
        if ' ext:' in commit.author:
            regex = re.compile('^(.*?)\ ext:\((.*)\) <(.*)\>$')
            m = regex.match(commit.author)
            if m:
                name = m.group(1)
                ex = urllib.unquote(m.group(2))
                email = m.group(3)
                author = name + ' <' + email + '>' + ex

        if ' <none@none>' in commit.author:
            author = commit.author[:-12]

        try:
            author.decode('utf-8')
        except UnicodeDecodeError:
            extra['author'] = author
            author = self.decode_guess(author, commit._encoding)

        oldenc = self.swap_out_encoding()

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

        gparents = map(self.map_hg_get, commit.parents)
        p1, p2 = (nullid, nullid)
        octopus = False

        if len(gparents) > 1:
            # merge, possibly octopus
            def commit_octopus(p1, p2):
                ctx = context.memctx(self.repo, (p1, p2), text, files, getfilectx,
                                     author, date, {'hg-git': 'octopus'})
                return hex(self.repo.commitctx(ctx))

            octopus = len(gparents) > 2
            p2 = gparents.pop()
            p1 = gparents.pop()
            while len(gparents) > 0:
                p2 = commit_octopus(p1, p2)
                p1 = gparents.pop()
        else:
            if gparents:
                p1 = gparents.pop()

        files = list(set(files))

        pa = None
        if not (p2 == nullid):
            node1 = self.repo.changectx(p1)
            node2 = self.repo.changectx(p2)
            pa = node1.ancestor(node2)

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

        if octopus:
            extra['hg-git'] ='octopus-done'

        ctx = context.memctx(self.repo, (p1, p2), text, files, getfilectx,
                             author, date, extra)

        node = self.repo.commit_import_ctx(ctx, pa)

        self.swap_out_encoding(oldenc)

        # save changeset to mapping file
        cs = hex(node)
        self.map_set(commit.id, cs)

    ## PACK UPLOADING AND FETCHING

    def upload_pack(self, remote):
        client, path = self.get_transport_and_path(remote)
        changed = self.get_changed_refs
        genpack = self.generate_pack_contents
        try:
            self.ui.status(_("creating and sending data\n"))
            changed_refs = client.send_pack(path, changed, genpack)
            return changed_refs
        except:
            # TODO: remove try/except or do something useful here
            raise

    # TODO: for now, we'll just push all heads that match remote heads
    #        * we should have specified push, tracking branches and --all
    # takes a dict of refs:shas from the server and returns what should be
    # pushed up
    def get_changed_refs(self, refs):
        keys = refs.keys()

        changed = {}
        if not keys:
            return None

        # TODO: this is a huge hack
        if keys[0] == 'capabilities^{}':
            # nothing on the server yet - first push
            if not 'master' in self.repo.tags():
                tip = self.repo.lookup('tip')
                changed['refs/heads/master'] = self.map_git_get(hex(tip))

        for tag, sha in self.tags.iteritems():
            tag_name = 'refs/tags/' + tag
            if tag_name not in refs:
                changed[tag_name] = self.map_git_get(sha)

        for ref_name in keys:
            parts = ref_name.split('/')
            if parts[0] == 'refs' and parts[1] == 'heads':
                # strip off 'refs/heads'
                head = "/".join([v for v in parts[2:]])
                try:
                    local_ref = self.repo.lookup(head)
                    remote_ref = self.map_hg_get(refs[ref_name])
                    if remote_ref:
                        remotectx = self.repo[remote_ref]
                        localctx = self.repo[local_ref]
                        if remotectx.ancestor(localctx) == remotectx:
                            # fast forward push
                            changed[ref_name] = self.map_git_get(hex(local_ref))
                        else:
                            # XXX: maybe abort completely
                            self.ui.warn('not pushing branch %s, please merge\n'% head)
                except RepoError: #pragma: no cover
                    # remote_ref is not here
                    pass

        # Also push any local branches not on the server yet
        for head in self.local_heads():
            ref = 'refs/heads/' + head
            if not ref in refs:
                node = self.repo.lookup(head)
                changed[ref] = self.map_git_get(hex(node))

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
        # TODO: don't send blobs or trees they already have
        def get_objects(tree, path):
            changes = list()
            changes.append((tree, path))
            for (mode, name, sha) in tree.entries():
                if mode == 0160000: # TODO: properly handle submodules and document what 57344 means
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
        client, path = self.get_transport_and_path(remote_name)
        graphwalker = SimpleFetchGraphWalker(self.git.heads().values(), self.git.get_parents)
        f, commit = self.git.object_store.add_pack()
        try:
            determine_wants = self.git.object_store.determine_wants_all
            refs = client.fetch_pack(path, determine_wants, graphwalker, f.write, sys.stdout.write)
            f.close()
            commit()
            return refs
        finally:
            f.close()

    ## REFERENCES HANDLING

    def update_references(self):
        heads = self.local_heads()

        # Create a local Git branch name for each
        # Mercurial bookmark.
        for key in heads:
            self.git.set_ref('refs/heads/' + key, heads[key])

    def export_hg_tags(self):
        for tag, sha in self.repo.tags().iteritems():
            if self.repo.tagtype(tag) in ('global', 'git'):
                self.git.set_ref('refs/tags/' + tag, self.map_git_get(hex(sha)))
                self.tags[tag] = hex(sha)

    def local_heads(self):
        try:
            bms = bookmarks.parse(self.repo)
            return dict([(bm, self.map_git_get(hex(bms[bm]))) for bm in bms])
        except AttributeError: #pragma: no cover
            return {}

    def import_tags(self, refs):
        keys = refs.keys()
        if not keys:
            return
        for k in keys[:]:
            ref_name = k
            parts = k.split('/')
            if parts[0] == 'refs' and parts[1] == 'tags':
                ref_name = "/".join([v for v in parts[2:]])
                if ref_name[-3:] == '^{}':
                    ref_name = ref_name[:-3]
                if not ref_name in self.repo.tags():
                    obj = self.git.get_object(refs[k])
                    sha = None
                    if isinstance (obj, Commit): # lightweight
                        sha = self.map_hg_get(refs[k])
                        self.tags[ref_name] = sha
                    elif isinstance (obj, Tag): # annotated
                        (obj_type, obj_sha) = obj.get_object()
                        obj = self.git.get_object(obj_sha)
                        if isinstance (obj, Commit):
                            sha = self.map_hg_get(obj_sha)
                            # TODO: better handling for annotated tags
                            self.tags[ref_name] = sha
        self.save_tags()

    def update_hg_bookmarks(self, refs):
        try:
            bms = bookmarks.parse(self.repo)
            heads = dict([(ref[11:],refs[ref]) for ref in refs
                          if ref.startswith('refs/heads/')])

            for head, sha in heads.iteritems():
                hgsha = bin(self.map_hg_get(sha))
                if not head in bms:
                    # new branch
                    bms[head] = hgsha
                else:
                    bm = self.repo[bms[head]]
                    if bm.ancestor(self.repo[hgsha]) == bm:
                        # fast forward
                        bms[head] = hgsha
            if heads:
                bookmarks.write(self.repo, bms)

        except AttributeError:
            self.ui.warn(_('creating bookmarks failed, do you have'
                         ' bookmarks enabled?\n'))

    def update_remote_branches(self, remote_name, refs):
        heads = dict([(ref[11:],refs[ref]) for ref in refs
                      if ref.startswith('refs/heads/')])

        for head, sha in heads.iteritems():
            hgsha = bin(self.map_hg_get(sha))
            tag = '%s/%s' % (remote_name, head)
            self.repo.tag(tag, hgsha, '', True, None, None)

        self.git.set_remote_refs(refs, remote_name)


    ## UTILITY FUNCTIONS

    def convert_git_int_mode(self, mode):
        # TODO: make these into constants
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
        extra = {}
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
                if command == 'extra':
                    before, after = data.split(" : ", 1)
                    extra[before] = urllib.unquote(after)
        return (message, renames, branch, extra)

    def remote_name(self, remote):
        names = [name for name, path in self.paths if path == remote]
        if names:
            return names[0]

    # Stolen from hgsubversion
    def swap_out_encoding(self, new_encoding='UTF-8'):
        try:
            from mercurial import encoding
            old = encoding.encoding
            encoding.encoding = new_encoding
        except ImportError:
            old = hgutil._encoding
            hgutil._encoding = new_encoding
        return old

    def decode_guess(self, string, encoding):
        # text is not valid utf-8, try to make sense of it
        if encoding:
            try:
                return string.decode(encoding).encode('utf-8')
            except UnicodeDecodeError:
                pass

        try:
            return string.decode('latin-1').encode('utf-8')
        except UnicodeDecodeError:
            return string.decode('ascii', 'replace').encode('utf-8')

    def get_transport_and_path(self, uri):
        from dulwich.client import TCPGitClient, SSHGitClient, SubprocessGitClient
        for handler, transport in (("git://", TCPGitClient), ("git@", SSHGitClient), ("git+ssh://", SSHGitClient)):
            if uri.startswith(handler):
                host, path = uri[len(handler):].split("/", 1)
                return transport(host), '/' + path
        # if its not git or git+ssh, try a local url..
        return SubprocessGitClient(), uri
