import os, math, urllib, re

from dulwich.errors import HangupException
from dulwich.index import commit_tree
from dulwich.objects import Blob, Commit, Tag, Tree, parse_timezone
from dulwich.pack import create_delta, apply_delta
from dulwich.repo import Repo
from dulwich import client

from hgext import bookmarks
from mercurial.i18n import _
from mercurial.node import hex, bin, nullid
from mercurial import context, util as hgutil
from mercurial import error


class GitHandler(object):
    mapfile = 'git-mapfile'
    tagsfile = 'git-tags'

    def __init__(self, dest_repo, ui):
        self.repo = dest_repo
        self.ui = ui

        if ui.config('git', 'intree'):
            self.gitdir = self.repo.wjoin('.git')
        else:
            self.gitdir = self.repo.join('git')

        self.paths = ui.configitems('paths')

        self.load_map()
        self.load_tags()

    # make the git data directory
    def init_if_missing(self):
        if os.path.exists(self.gitdir):
            self.git = Repo(self.gitdir)
        else:
            os.mkdir(self.gitdir)
            self.git = Repo.init_bare(self.gitdir)

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
        for hgsha, gitsha in sorted(self._map_hg.iteritems()):
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

    def fetch(self, remote, heads):
        self.export_commits()
        refs = self.fetch_pack(remote, heads)
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

    def export_commits(self):
        try:
            self.export_git_objects()
            self.export_hg_tags()
            self.update_references()
        finally:
            self.save_map()

    def get_refs(self, remote):
        self.export_commits()
        client, path = self.get_transport_and_path(remote)
        old_refs = {}
        new_refs = {}
        def changed(refs):
            old_refs.update(refs)
            to_push = set(self.local_heads().values() + self.tags.values())
            new_refs.update(self.get_changed_refs(refs, to_push, True))
            # don't push anything
            return {}

        try:
            client.send_pack(path, changed, None)

            changed_refs = [ref for ref, sha in new_refs.iteritems()
                            if sha != old_refs.get(ref)]
            new = [bin(self.map_hg_get(new_refs[ref])) for ref in changed_refs]
            old = dict( (bin(self.map_hg_get(old_refs[r])), 1)
                       for r in changed_refs if r in old_refs)

            return old, new
        except HangupException:
            raise hgutil.Abort("the remote end hung up unexpectedly")

    def push(self, remote, revs, force):
        self.export_commits()
        changed_refs = self.upload_pack(remote, revs, force)
        remote_name = self.remote_name(remote)

        if remote_name and changed_refs:
            for ref, sha in changed_refs.iteritems():
                self.ui.status("    %s::%s => GIT:%s\n" %
                               (remote_name, ref, sha[0:8]))

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
        self.ui.status(_("importing Hg objects into Git\n"))
        self.init_if_missing()

        nodes = [self.repo.lookup(n) for n in self.repo]
        export = [node for node in nodes if not hex(node) in self._map_hg]
        total = len(export)
        for i, rev in enumerate(export):
            self.ui.progress('import', i, total=total)
            ctx = self.repo.changectx(rev)
            state = ctx.extra().get('hg-git', None)
            if state == 'octopus':
                self.ui.debug("revision %d is a part "
                              "of octopus explosion\n" % ctx.rev())
                continue
            self.export_hg_commit(rev)
        self.ui.progress('import', None, total=total)


    # convert this commit into git objects
    # go through the manifest, convert all blobs/trees we don't have
    # write the commit object (with metadata info)
    def export_hg_commit(self, rev):
        self.ui.note(_("converting revision %s\n") % hex(rev))

        oldenc = self.swap_out_encoding()

        ctx = self.repo.changectx(rev)
        extra = ctx.extra()

        commit = Commit()

        (time, timezone) = ctx.date()
        commit.author = self.get_git_author(ctx)
        commit.author_time = int(time)
        commit.author_timezone = -timezone

        if 'committer' in extra:
            # fixup timezone
            (name, timestamp, timezone) = extra['committer'].rsplit(' ', 2)
            commit.committer = name
            commit.commit_time = timestamp

            # work around a timezone format change
            if int(timezone) % 60 != 0: #pragma: no cover
                timezone = parse_timezone(timezone)
            else:
                timezone = -int(timezone)
            commit.commit_timezone = timezone
        else:
            commit.committer = commit.author
            commit.commit_time = commit.author_time
            commit.commit_timezone = commit.author_timezone

        commit.parents = []
        for parent in self.get_git_parents(ctx):
            hgsha = hex(parent.node())
            git_sha = self.map_git_get(hgsha)
            if git_sha:
                commit.parents.append(git_sha)

        commit.message = self.get_git_message(ctx)

        if 'encoding' in extra:
            commit.encoding = extra['encoding']

        tree_sha = commit_tree(self.git.object_store, self.iterblobs(ctx))
        commit.tree = tree_sha

        self.git.object_store.add_object(commit)
        self.map_set(commit.id, ctx.hex())

        self.swap_out_encoding(oldenc)
        return commit.id

    def get_git_author(self, ctx):
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

        if 'author' in ctx.extra():
            author = apply_delta(author, ctx.extra()['author'])

        return author

    def get_git_parents(self, ctx):
        def is_octopus_part(ctx):
            return ctx.extra().get('hg-git', None) in ('octopus', 'octopus-done')

        parents = []
        if ctx.extra().get('hg-git', None) == 'octopus-done':
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

        return parents

    def get_git_message(self, ctx):
        extra = ctx.extra()

        message = ctx.description() + "\n"
        if 'message' in extra:
            message = apply_delta(message, extra['message'])

        # HG EXTRA INFORMATION
        add_extras = False
        extra_message = ''
        if not ctx.branch() == 'default':
            add_extras = True
            extra_message += "branch : " + ctx.branch() + "\n"

        renames = []
        for f in ctx.files():
            if f not in ctx.manifest():
                continue
            rename = ctx.filectx(f).renamed()
            if rename:
                renames.append((rename[0], f))

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
            message += "\n--HG--\n" + extra_message

        return message

    def iterblobs(self, ctx):
        for f in ctx:
            fctx = ctx[f]
            blobid = self.map_git_get(hex(fctx.filenode()))

            if not blobid:
                blob = Blob.from_string(fctx.data())
                self.git.object_store.add_object(blob)
                self.map_set(blob.id, hex(fctx.filenode()))
                blobid = blob.id

            if 'l' in ctx.flags(f):
                mode = 0120000
            elif 'x' in ctx.flags(f):
                mode = 0100755
            else:
                mode = 0100644

            yield f, blobid, mode

    def import_git_objects(self, remote_name=None, refs=None):
        self.ui.status(_("importing Git objects into Hg\n"))
        self.init_if_missing()

        # import heads and fetched tags as remote references
        todo = []
        done = set()
        convert_list = {}

        # get a list of all the head shas
        seenheads = set()
        if refs is None:
            refs = self.git.refs.as_dict()
        if refs:
            for sha in refs.itervalues():
                # refs contains all the refs in the server, not just the ones
                # we are pulling
                if sha in self.git.object_store:
                    obj = self.git.get_object(sha)
                    while isinstance(obj, Tag):
                        obj_type, sha = obj.get_object()
                        obj = self.git.get_object(sha)
                    if isinstance (obj, Commit) and sha not in seenheads:
                        seenheads.add(sha)
                        todo.append(sha)

        # sort by commit date
        def commitdate(sha):
            obj = self.git.get_object(sha)
            return obj.commit_time-obj.commit_timezone

        todo.sort(key=commitdate, reverse=True)

        # traverse the heads getting a list of all the unique commits
        commits = []
        seen = set(todo)
        while todo:
            sha = todo[-1]
            if sha in done:
                todo.pop()
                continue
            assert isinstance(sha, str)
            obj = self.git.get_object(sha)
            assert isinstance(obj, Commit)
            for p in obj.parents:
                if p not in done:
                    todo.append(p)
                    break
            else:
                commits.append(sha)
                convert_list[sha] = obj
                done.add(sha)
                todo.pop()

        commits = [commit for commit in commits if not commit in self._map_git]
        # import each of the commits, oldest first
        total = len(commits)
        for i, csha in enumerate(commits):
            self.ui.progress('import', i, total=total, unit='commits')
            commit = convert_list[csha]
            self.import_git_commit(commit)
        self.ui.progress('import', None, total=total, unit='commits')

    def import_git_commit(self, commit):
        self.ui.debug(_("importing: %s\n") % commit.id)

        (strip_message, hg_renames,
         hg_branch, extra) = self.extract_hg_metadata(commit.message)

        # get a list of the changed, added, removed files
        files = self.get_files_changed(commit)

        date = (commit.author_time, -commit.author_timezone)
        text = strip_message

        origtext = text
        try:
            text.decode('utf-8')
        except UnicodeDecodeError:
            text = self.decode_guess(text, commit.encoding)

        text = '\n'.join([l.rstrip() for l in text.splitlines()]).strip('\n')
        if text + '\n' != origtext:
            extra['message'] = create_delta(text +'\n', origtext)

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
            origauthor = author
            author = self.decode_guess(author, commit.encoding)
            extra['author'] = create_delta(author, origauthor)

        oldenc = self.swap_out_encoding()

        def getfilectx(repo, memctx, f):
            delete, mode, sha = files[f]
            if delete:
                raise IOError

            data = self.git[sha].data
            copied_path = hg_renames.get(f)
            e = self.convert_git_int_mode(mode)

            return context.memfilectx(f, data, 'l' in e, 'x' in e, copied_path)

        gparents = map(self.map_hg_get, commit.parents)
        p1, p2 = (nullid, nullid)
        octopus = False

        if len(gparents) > 1:
            # merge, possibly octopus
            def commit_octopus(p1, p2):
                ctx = context.memctx(self.repo, (p1, p2), text, list(files), getfilectx,
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

        pa = None
        if not (p2 == nullid):
            node1 = self.repo.changectx(p1)
            node2 = self.repo.changectx(p2)
            pa = node1.ancestor(node2)

        # if named branch, add to extra
        if hg_branch:
            extra['branch'] = hg_branch

        # if committer is different than author, add it to extra
        if commit.author != commit.committer \
               or commit.author_time != commit.commit_time \
               or commit.author_timezone != commit.commit_timezone:
            extra['committer'] = "%s %d %d" % (
                commit.committer, commit.commit_time, -commit.commit_timezone)

        if commit.encoding:
            extra['encoding'] = commit.encoding

        if hg_branch:
            extra['branch'] = hg_branch

        if octopus:
            extra['hg-git'] ='octopus-done'

        # TODO use 'n in self.repo' when we require hg 1.5
        def repo_contains(n):
            try:
                return bool(self.repo.lookup(n))
            except error.RepoLookupError:
                return False

        if not (repo_contains(p1) and repo_contains(p2)):
            raise hgutil.Abort(_('you appear to have run strip - '
                                 'please run hg git-cleanup'))
        ctx = context.memctx(self.repo, (p1, p2), text, list(files), getfilectx,
                             author, date, extra)

        node = self.repo.commitctx(ctx)

        self.swap_out_encoding(oldenc)

        # save changeset to mapping file
        cs = hex(node)
        self.map_set(commit.id, cs)

    ## PACK UPLOADING AND FETCHING

    def upload_pack(self, remote, revs, force):
        client, path = self.get_transport_and_path(remote)
        def changed(refs):
            to_push = revs or set(self.local_heads().values() + self.tags.values())
            return self.get_changed_refs(refs, to_push, force)

        genpack = self.git.object_store.generate_pack_contents
        try:
            self.ui.status(_("creating and sending data\n"))
            changed_refs = client.send_pack(path, changed, genpack)
            return changed_refs
        except HangupException:
            raise hgutil.Abort("the remote end hung up unexpectedly")

    def get_changed_refs(self, refs, revs, force):
        new_refs = refs.copy()

        #The remote repo is empty and the local one doesn't have bookmarks/tags
        if refs.keys()[0] == 'capabilities^{}':
            del new_refs['capabilities^{}']
            if not self.local_heads():
                tip = hex(self.repo.lookup('tip'))
                bookmarks.bookmark(self.ui, self.repo, 'master', tip, force=True)
                bookmarks.setcurrent(self.repo, 'master')
                new_refs['refs/heads/master'] = self.map_git_get(tip)

        for rev in revs:
            ctx = self.repo[rev]
            heads = [t for t in ctx.tags() if t in self.local_heads()]
            tags = [t for t in ctx.tags() if t in self.tags]

            if not (heads or tags):
                raise hgutil.Abort("revision %s cannot be pushed since"
                                   " it doesn't have a ref" % ctx)

            # Check if the tags the server is advertising are annotated tags,
            # by attempting to retrieve it from the our git repo, and building a
            # list of these tags.
            #
            # This is possible, even though (currently) annotated tags are
            # dereferenced and stored as lightweight ones, as the annotated tag
            # is still stored in the git repo.
            uptodate_annotated_tags = []
            for r in tags:
                ref = 'refs/tags/'+r
                # Check tag.
                if not ref in refs:
                    continue
                try:
                    # We're not using Repo.tag(), as it's deprecated.
                    tag = self.git.get_object(refs[ref])
                    if not isinstance(tag, Tag):
                        continue
                except KeyError:
                    continue

                # If we've reached here, the tag's good.
                uptodate_annotated_tags.append(ref)

            for r in heads + tags:
                if r in heads:
                    ref = 'refs/heads/'+r
                else:
                    ref = 'refs/tags/'+r

                if ref not in refs:
                    new_refs[ref] = self.map_git_get(ctx.hex())
                elif new_refs[ref] in self._map_git:
                    rctx = self.repo[self.map_hg_get(new_refs[ref])]
                    if rctx.ancestor(ctx) == rctx or force:
                        new_refs[ref] = self.map_git_get(ctx.hex())
                    else:
                        raise hgutil.Abort("pushing %s overwrites %s"
                                           % (ref, ctx))
                elif ref in uptodate_annotated_tags:
                    # we already have the annotated tag.
                    pass
                else:
                    raise hgutil.Abort("%s changed on the server, please pull "
                                       "and merge before pushing" % ref)

        return new_refs


    def fetch_pack(self, remote_name, heads):
        client, path = self.get_transport_and_path(remote_name)
        graphwalker = self.git.get_graph_walker()
        def determine_wants(refs):
            if heads:
                want = []
                for h in heads:
                    r = [ref for ref in refs if ref.endswith('/'+h)]
                    if not r:
                        raise hgutil.Abort("ref %s not found on remote server" % h)
                    elif len(r) == 1:
                        want.append(refs[r[0]])
                    else:
                        raise hgutil.Abort("ambiguous reference %s: %r" % (h, r))
            else:
                want = [sha for ref, sha in refs.iteritems()
                        if not ref.endswith('^{}')]
            return want
        f, commit = self.git.object_store.add_pack()
        try:
            try:
                return client.fetch_pack(path, determine_wants, graphwalker,
                                         f.write, self.ui.status)
            except HangupException:
                raise hgutil.Abort("the remote end hung up unexpectedly")
        finally:
            commit()

    ## REFERENCES HANDLING

    def update_references(self):
        heads = self.local_heads()

        # Create a local Git branch name for each
        # Mercurial bookmark.
        for key in heads:
            self.git.refs['refs/heads/' + key] = self.map_git_get(heads[key])

    def export_hg_tags(self):
        for tag, sha in self.repo.tags().iteritems():
            if self.repo.tagtype(tag) in ('global', 'git'):
                self.git.refs['refs/tags/' + tag] = self.map_git_get(hex(sha))
                self.tags[tag] = hex(sha)

    def local_heads(self):
        try:
            if getattr(bookmarks, 'parse', None):
                bms = bookmarks.parse(self.repo)
            else:
                bms = self.repo._bookmarks
            return dict([(bm, hex(bms[bm])) for bm in bms])
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
                # refs contains all the refs in the server, not just
                # the ones we are pulling
                if refs[k] not in self.git.object_store:
                    continue
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
            oldbm = getattr(bookmarks, 'parse', None)
            if oldbm:
                bms = bookmarks.parse(self.repo)
            else:
                bms = self.repo._bookmarks
            heads = dict([(ref[11:],refs[ref]) for ref in refs
                          if ref.startswith('refs/heads/')])

            for head, sha in heads.iteritems():
                # refs contains all the refs in the server, not just
                # the ones we are pulling
                if sha not in self.git.object_store:
                    continue
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
                if oldbm:
                    bookmarks.write(self.repo, bms)
                else:
                    self.repo._bookmarks = bms
                    bookmarks.write(self.repo)

        except AttributeError:
            self.ui.warn(_('creating bookmarks failed, do you have'
                         ' bookmarks enabled?\n'))

    def update_remote_branches(self, remote_name, refs):
        heads = dict([(ref[11:],refs[ref]) for ref in refs
                      if ref.startswith('refs/heads/')])

        for head, sha in heads.iteritems():
            # refs contains all the refs in the server, not just the ones
            # we are pulling
            if sha not in self.git.object_store:
                continue
            hgsha = bin(self.map_hg_get(sha))
            tag = '%s/%s' % (remote_name, head)
            self.repo.tag(tag, hgsha, '', True, None, None)

        for ref_name in refs:
            if ref_name.startswith('refs/heads'):
                new_ref = 'refs/remotes/%s/%s' % (remote_name, ref_name[10:])
                self.git.refs[new_ref] = refs[ref_name]
            elif ref_name.startswith('refs/tags'):
                self.git.refs[ref_name] = refs[ref_name]


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
        split = message.split("\n--HG--\n", 1)
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

    def get_file(self, commit, f):
        otree = self.git.tree(commit.tree)
        parts = f.split('/')
        for part in parts:
            (mode, sha) = otree[part]
            obj = self.git.get_object(sha)
            if isinstance (obj, Blob):
                return (mode, sha, obj._text)
            elif isinstance(obj, Tree):
                otree = obj

    def get_files_changed(self, commit):
        tree = commit.tree
        btree = None

        if commit.parents:
            btree = self.git[commit.parents[0]].tree

        changes = self.git.object_store.tree_changes(btree, tree)
        files = {}
        for (oldfile, newfile), (oldmode, newmode), (oldsha, newsha) in changes:
            # don't create new submodules
            if newmode == 0160000:
                if oldfile:
                    # become a regular delete
                    newfile, newmode = None, None
                else:
                    continue
            # so old submodules shoudn't exist
            if oldmode == 0160000:
                if newfile:
                    # become a regular add
                    oldfile, oldmode = None, None
                else:
                    continue

            if newfile is None:
                file = oldfile
                delete = True
            else:
                file = newfile
                delete = False

            files[file] = (delete, newmode, newsha)

        return files

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
        for handler, transport in (("git://", client.TCPGitClient),
                                   ("git@", client.SSHGitClient),
                                   ("git+ssh://", client.SSHGitClient)):
            if uri.startswith(handler):
                # We need to split around : or /, whatever comes first
                hostpath = uri[len(handler):]
                if (hostpath.find(':') > 0 and hostpath.find('/') > 0):
                    # we have both, whatever is first wins.
                    if hostpath.find(':') < hostpath.find('/'):
                      hostpath_seper = ':'
                    else:
                      hostpath_seper = '/'
                elif hostpath.find(':') > 0:
                    hostpath_seper = ':'
                else:
                    hostpath_seper = '/'

                host, path = hostpath.split(hostpath_seper, 1)
                if hostpath_seper == '/':
                    transportpath = '/' + path
                else:
                    transportpath = path
                return transport(host, thin_packs=False), transportpath
        # if its not git or git+ssh, try a local url..
        return client.SubprocessGitClient(thin_packs=False), uri
