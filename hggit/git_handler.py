import os, math, urllib, re
import stat, posixpath, StringIO
import ordereddict

from dulwich.errors import HangupException, GitProtocolError
from dulwich.index import commit_tree
from dulwich.objects import Blob, Commit, Tag, Tree, parse_timezone, S_IFGITLINK
from dulwich.pack import create_delta, apply_delta
from dulwich.repo import Repo
from dulwich import client
from dulwich import config as dul_config

try:
    from mercurial import bookmarks
    bookmarks.update
    from mercurial import commands
except ImportError:
    from hgext import bookmarks
try:
    from mercurial.error import RepoError
except ImportError:
    from mercurial.repo import RepoError

from mercurial.i18n import _
from mercurial.node import hex, bin, nullid
from mercurial import context, util as hgutil
from mercurial import error

import _ssh
import util
from overlay import overlayrepo

class GitProgress(object):
    """convert git server progress strings into mercurial progress"""
    def __init__(self, ui):
        self.ui = ui

        self.lasttopic = None
        self.msgbuf = ''

    def progress(self, msg):
        # 'Counting objects: 33640, done.\n'
        # 'Compressing objects:   0% (1/9955)   \r
        msgs = re.split('[\r\n]', self.msgbuf + msg)
        self.msgbuf = msgs.pop()

        for msg in msgs:
            td = msg.split(':', 1)
            data = td.pop()
            if not td:
                self.flush(data)
                continue
            topic = td[0]

            m = re.search('\((\d+)/(\d+)\)', data)
            if m:
                if self.lasttopic and self.lasttopic != topic:
                    self.flush()
                self.lasttopic = topic

                pos, total = map(int, m.group(1, 2))
                util.progress(self.ui, topic, pos, total=total)
            else:
                self.flush(msg)

    def flush(self, msg=None):
        if self.lasttopic:
            util.progress(self.ui, self.lasttopic, None)
        self.lasttopic = None
        if msg:
            self.ui.note(msg + '\n')

class GitHandler(object):
    mapfile = 'git-mapfile'
    tagsfile = 'git-tags'

    def __init__(self, dest_repo, ui):
        self.repo = dest_repo
        self.ui = ui

        if ui.configbool('git', 'intree'):
            self.gitdir = self.repo.wjoin('.git')
        else:
            self.gitdir = self.repo.join('git')

        self.init_author_file()

        self.paths = ui.configitems('paths')

        self.branch_bookmark_suffix = ui.config('git', 'branch_bookmark_suffix')

        self.load_map()
        self.load_tags()

    # make the git data directory
    def init_if_missing(self):
        if os.path.exists(self.gitdir):
            self.git = Repo(self.gitdir)
        else:
            os.mkdir(self.gitdir)
            self.git = Repo.init_bare(self.gitdir)

    def init_author_file(self):
        self.author_map = {}
        if self.ui.config('git', 'authors'):
            with open(self.repo.wjoin(
                self.ui.config('git', 'authors')
            )) as f:
                for line in f:
                    line = line.strip()
                    if not line or line.startswith('#'):
                        continue
                    from_, to = re.split(r'\s*=\s*', line, 2)
                    self.author_map[from_] = to

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
        # If this complains that NoneType is not callable, then
        # atomictempfile no longer has either of rename (pre-1.9) or
        # close (post-1.9)
        getattr(file, 'rename', getattr(file, 'close', None))()

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
        # If this complains that NoneType is not callable, then
        # atomictempfile no longer has either of rename (pre-1.9) or
        # close (post-1.9)
        getattr(file, 'rename', getattr(file, 'close', None))()

    ## END FILE LOAD AND SAVE METHODS

    ## COMMANDS METHODS

    def import_commits(self, remote_name):
        self.import_git_objects(remote_name)
        self.update_hg_bookmarks(self.git.get_refs())
        self.save_map()

    def fetch(self, remote, heads):
        self.export_commits()
        refs = self.fetch_pack(remote, heads)
        remote_name = self.remote_name(remote)

        oldrefs = self.git.get_refs()
        if refs:
            self.import_git_objects(remote_name, refs)
            self.import_tags(refs)
            self.update_hg_bookmarks(refs)
            if remote_name:
                self.update_remote_branches(remote_name, refs)
            elif not self.paths:
                # intial cloning
                self.update_remote_branches('default', refs)

                # "Activate" a tipmost bookmark.
                bms = getattr(self.repo['tip'], 'bookmarks',
                              lambda : None)()
                if bms:
                    bookmarks.setcurrent(self.repo, bms[0])

        def remoteref(ref):
            rn = remote_name or 'default'
            return 'refs/remotes/' + rn + ref[10:]

        modheads = [refs[k] for k in refs if k.startswith('refs/heads/')
                    and not k.endswith('^{}')
                    and refs[k] != oldrefs.get(remoteref(k))]

        if not modheads:
            self.ui.status(_("no changes found\n"))

        self.save_map()

        return len(modheads)

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
            client.send_pack(path, changed, lambda have, want: [])

            changed_refs = [ref for ref, sha in new_refs.iteritems()
                            if sha != old_refs.get(ref)]
            new = [bin(self.map_hg_get(new_refs[ref])) for ref in changed_refs]
            old = {}
            for r in old_refs:
                old_ref = self.map_hg_get(old_refs[r])
                if old_ref:
                    old[bin(old_ref)] = 1

            return old, new
        except (HangupException, GitProtocolError), e:
            raise hgutil.Abort(_("git remote error: ") + str(e))

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

    # incoming support
    def getremotechanges(self, remote, revs):
        self.export_commits()
        refs = self.fetch_pack(remote.path, revs)

        # refs contains all remote refs. Prune to only those requested.
        if revs:
            reqrefs = {}
            for rev in revs:
                for n in ('refs/heads/' + rev, 'refs/tags/' + rev):
                    if n in refs:
                        reqrefs[n] = refs[n]
        else:
            reqrefs = refs

        commits = [bin(c) for c in self.getnewgitcommits(reqrefs)[1]]

        b = overlayrepo(self, commits, refs)

        return (b, commits, lambda: None)

    ## CHANGESET CONVERSION METHODS

    def export_git_objects(self):
        self.init_if_missing()

        nodes = [self.repo.lookup(n) for n in self.repo]
        export = [node for node in nodes if not hex(node) in self._map_hg]
        total = len(export)
        if total:
            self.ui.status(_("exporting hg objects to git\n"))
        for i, rev in enumerate(export):
            util.progress(self.ui, 'exporting', i, total=total)
            ctx = self.repo.changectx(rev)
            state = ctx.extra().get('hg-git', None)
            if state == 'octopus':
                self.ui.debug("revision %d is a part "
                              "of octopus explosion\n" % ctx.rev())
                continue
            self.export_hg_commit(rev)
        util.progress(self.ui, 'importing', None, total=total)


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
                # Newer versions of Dulwich return a tuple here
                if isinstance(timezone, tuple):
                    timezone, neg_utc = timezone
                    commit._commit_timezone_neg_utc = neg_utc
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

    def get_valid_git_username_email(self, name):
        r"""Sanitize usernames and emails to fit git's restrictions.

        The following is taken from the man page of git's fast-import
        command:

            [...] Likewise LF means one (and only one) linefeed [...]

            committer
                The committer command indicates who made this commit,
                and when they made it.

                Here <name> is the person's display name (for example
                "Com M Itter") and <email> is the person's email address
                ("cm@example.com[1]"). LT and GT are the literal
                less-than (\x3c) and greater-than (\x3e) symbols. These
                are required to delimit the email address from the other
                fields in the line. Note that <name> and <email> are
                free-form and may contain any sequence of bytes, except
                LT, GT and LF. <name> is typically UTF-8 encoded.

        Accordingly, this function makes sure that there are none of the
        characters <, >, or \n in any string which will be used for
        a git username or email. Before this, it first removes left
        angle brackets and spaces from the beginning, and right angle
        brackets and spaces from the end, of this string, to convert
        such things as " <john@doe.com> " to "john@doe.com" for
        convenience.

        TESTS:

        >>> from mercurial.ui import ui
        >>> g = GitHandler('', ui()).get_valid_git_username_email
        >>> g('John Doe')
        'John Doe'
        >>> g('john@doe.com')
        'john@doe.com'
        >>> g(' <john@doe.com> ')
        'john@doe.com'
        >>> g('    <random<\n<garbage\n>  > > ')
        'random???garbage?'
        >>> g('Typo in hgrc >but.hg-git@handles.it.gracefully>')
        'Typo in hgrc ?but.hg-git@handles.it.gracefully'
        """
        return re.sub('[<>\n]', '?', name.lstrip('< ').rstrip('> '))

    def get_git_author(self, ctx):
        # hg authors might not have emails
        author = ctx.user()

        # see if a translation exists
        if author in self.author_map:
            author = self.author_map[author]

        # check for git author pattern compliance
        regex = re.compile('^(.*?) ?\<(.*?)(?:\>(.*))?$')
        a = regex.match(author)

        if a:
            name = self.get_valid_git_username_email(a.group(1))
            email = self.get_valid_git_username_email(a.group(2))
            if a.group(3) != None and len(a.group(3)) != 0:
                name += ' ext:(' + urllib.quote(a.group(3)) + ')'
            author = self.get_valid_git_username_email(name) + ' <' + self.get_valid_git_username_email(email) + '>'
        elif '@' in author:
            author = self.get_valid_git_username_email(author) + ' <' + self.get_valid_git_username_email(author) + '>'
        else:
            author = self.get_valid_git_username_email(author) + ' <none@none>'

        if 'author' in ctx.extra():
            author = "".join(apply_delta(author, ctx.extra()['author']))

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
            message = "".join(apply_delta(message, extra['message']))

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
        if '.hgsubstate' in ctx:
            hgsub = ordereddict.OrderedDict()
            if '.hgsub' in ctx:
                hgsub = util.parse_hgsub(ctx['.hgsub'].data().splitlines())
            hgsubstate = util.parse_hgsubstate(ctx['.hgsubstate'].data().splitlines())
            for path, sha in hgsubstate.iteritems():
                try:
                    if path in hgsub and not hgsub[path].startswith('[git]'):
                        # some other kind of a repository (e.g. [hg])
                        # that keeps its state in .hgsubstate, shall ignore
                        continue
                    yield path, sha, S_IFGITLINK
                except ValueError:
                    pass

        for f in ctx:
            if f == '.hgsubstate' or f == '.hgsub':
                continue
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

    def getnewgitcommits(self, refs=None):
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
                        obj_type, sha = obj.object
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

        return convert_list, [commit for commit in commits if not commit in self._map_git]

    def import_git_objects(self, remote_name=None, refs=None):
        convert_list, commits = self.getnewgitcommits(refs)
        # import each of the commits, oldest first
        total = len(commits)
        if total:
            self.ui.status(_("importing git objects into hg\n"))

        for i, csha in enumerate(commits):
            util.progress(self.ui, 'importing', i, total=total, unit='commits')
            commit = convert_list[csha]
            self.import_git_commit(commit)
        util.progress(self.ui, 'importing', None, total=total, unit='commits')

        # Remove any dangling tag references.
        for name, rev in self.repo.tags().items():
            if not rev in self.repo:
                if hasattr(self, 'tagscache') and self.tagscache and \
                        'name' in self.tagscache:
                    # Mercurial 1.4 and earlier.
                    del self.repo.tagscache[name]
                elif hasattr(self, '_tags') and self._tags and \
                        'name' in self._tags:
                    # Mercurial 1.5 and later.
                    del self.repo._tags[name]
                if (hgutil.safehasattr(self.repo, '_tagtypes') and
                    self.repo._tagtypes and
                    name in self.repo._tagtypes):
                    # Mercurial 1.9 and earlier.
                    del self.repo._tagtypes[name]
                elif (hgutil.safehasattr(self.repo, 'tagscache') and
                      self.repo.tagscache and
                      hgutil.safehasattr(self.repo.tagscache, '_tagtypes') and
                      self.repo.tagscache._tagtypes and
                      name in self.repo.tagscache._tagtypes):
                    # Mercurial 2.0 and later.
                    del self.repo.tagscache._tagtypes[name]

    def import_git_commit(self, commit):
        self.ui.debug(_("importing: %s\n") % commit.id)

        (strip_message, hg_renames,
         hg_branch, extra) = self.extract_hg_metadata(commit.message)

        # get a list of the changed, added, removed files
        files = self.get_files_changed(commit)

        # Handle gitlinks: collect
        gitlinks = self.collect_gitlinks(commit.tree)
        git_commit_tree = self.git[commit.tree]

        # Analyze hgsubstate and build an updated version
        # using SHAs from gitlinks
        hgsubstate = None
        if gitlinks:
            hgsubstate = util.parse_hgsubstate(self.git_file_readlines(git_commit_tree, '.hgsubstate'))
            for path, sha in gitlinks:
                hgsubstate[path] = sha
            # in case .hgsubstate wasn't among changed files
            # force its inclusion
            files['.hgsubstate'] = (False, 0100644, None)

        # Analyze .hgsub and merge with .gitmodules
        hgsub = None
        gitmodules = self.parse_gitmodules(git_commit_tree)
        if gitmodules or gitlinks:
            hgsub = util.parse_hgsub(self.git_file_readlines(git_commit_tree, '.hgsub'))
            for (sm_path, sm_url, sm_name) in gitmodules:
                hgsub[sm_path] = '[git]' + sm_url
            files['.hgsub'] = (False, 0100644, None)
        elif commit.parents and '.gitmodules' in self.git[self.git[commit.parents[0]].tree]:
            # no .gitmodules in this commit, however present in the parent
            # mark its hg counterpart as deleted (assuming .hgsub is there
            # due to the same import_git_commit process
            files['.hgsub'] = (True, 0100644, None)

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

        def findconvergedfiles(p1, p2):
            # If any files have the same contents in both parents of a merge
            # (and are therefore not reported as changed by Git) but are at
            # different file revisions in Mercurial (because they arrived at
            # those contents in different ways), we need to include them in
            # the list of changed files so that Mercurial can join up their
            # filelog histories (same as if the merge was done in Mercurial to
            # begin with).
            if p2 == nullid:
                return []
            manifest1 = self.repo.changectx(p1).manifest()
            manifest2 = self.repo.changectx(p2).manifest()
            return [path for path, node1 in manifest1.iteritems()
                    if path not in files and manifest2.get(path, node1) != node1]

        def getfilectx(repo, memctx, f):
            info = files.get(f)
            if info != None:
                # it's a file reported as modified from Git
                delete, mode, sha = info
                if delete:
                    raise IOError

                if not sha: # indicates there's no git counterpart
                    e = ''
                    copied_path = None
                    if '.hgsubstate' == f:
                        data = util.serialize_hgsubstate(hgsubstate)
                    elif '.hgsub' == f:
                        data = util.serialize_hgsub(hgsub)
                else:
                    data = self.git[sha].data
                    copied_path = hg_renames.get(f)
                    e = self.convert_git_int_mode(mode)
            else:
                # it's a converged file
                fc = context.filectx(self.repo, f, changeid=memctx.p1().rev())
                data = fc.data()
                e = fc.flags()
                copied_path = fc.renamed()

            return context.memfilectx(f, data, 'l' in e, 'x' in e, copied_path)

        gparents = map(self.map_hg_get, commit.parents)
        p1, p2 = (nullid, nullid)
        octopus = False

        if len(gparents) > 1:
            # merge, possibly octopus
            def commit_octopus(p1, p2):
                ctx = context.memctx(self.repo, (p1, p2), text,
                                     list(files) + findconvergedfiles(p1, p2),
                                     getfilectx, author, date, {'hg-git': 'octopus'})
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
        ctx = context.memctx(self.repo, (p1, p2), text,
                             list(files) + findconvergedfiles(p1, p2),
                             getfilectx, author, date, extra)

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
        except (HangupException, GitProtocolError), e:
            raise hgutil.Abort(_("git remote error: ") + str(e))

    def get_changed_refs(self, refs, revs, force):
        new_refs = refs.copy()

        #The remote repo is empty and the local one doesn't have bookmarks/tags
        if refs.keys()[0] == 'capabilities^{}':
            del new_refs['capabilities^{}']
            if not self.local_heads():
                tip = hex(self.repo.lookup('tip'))
                try:
                    commands.bookmark(self.ui, self.repo, 'master', tip, force=True)
                except NameError:
                    bookmarks.bookmark(self.ui, self.repo, 'master', tip, force=True)
                bookmarks.setcurrent(self.repo, 'master')
                new_refs['refs/heads/master'] = self.map_git_get(tip)

        for rev in revs:
            ctx = self.repo[rev]
            if getattr(ctx, 'bookmarks', None):
                labels = lambda c: ctx.tags() + [
                                fltr for fltr, bm
                                in self._filter_for_bookmarks(ctx.bookmarks())
                            ]
            else:
                labels = lambda c: ctx.tags()
            prep = lambda itr: [i.replace(' ', '_') for i in itr]

            heads = [t for t in prep(labels(ctx)) if t in self.local_heads()]
            tags = [t for t in prep(labels(ctx)) if t in self.tags]

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
                # contains pairs of ('refs/(heads|tags|...)/foo', 'foo')
                # if ref is just '<foo>', then we get ('foo', 'foo')
                stripped_refs = [
                    (r, r[r.find('/', r.find('/')+1)+1:])
                        for r in refs]
                for h in heads:
                    r = [pair[0] for pair in stripped_refs if pair[1] == h]
                    if not r:
                        raise hgutil.Abort("ref %s not found on remote server" % h)
                    elif len(r) == 1:
                        want.append(refs[r[0]])
                    else:
                        raise hgutil.Abort("ambiguous reference %s: %r" % (h, r))
            else:
                want = [sha for ref, sha in refs.iteritems()
                        if not ref.endswith('^{}')
                        and ( ref.startswith('refs/heads/') or ref.startswith('refs/tags/') ) ]
            want = [x for x in want if x not in self.git]
            return want
        f, commit = self.git.object_store.add_pack()
        try:
            try:
                progress = GitProgress(self.ui)
                ret = client.fetch_pack(path, determine_wants, graphwalker,
                                        f.write, progress.progress)
                progress.flush()
                return ret
            except (HangupException, GitProtocolError), e:
                raise hgutil.Abort(_("git remote error: ") + str(e))
        finally:
            commit()

    ## REFERENCES HANDLING

    def update_references(self):
        heads = self.local_heads()

        # Create a local Git branch name for each
        # Mercurial bookmark.
        for key in heads:
            git_ref = self.map_git_get(heads[key])
            if git_ref:
                self.git.refs['refs/heads/' + key] = self.map_git_get(heads[key])

    def export_hg_tags(self):
        for tag, sha in self.repo.tags().iteritems():
            if self.repo.tagtype(tag) in ('global', 'git'):
                tag = tag.replace(' ', '_')
                self.git.refs['refs/tags/' + tag] = self.map_git_get(hex(sha))
                self.tags[tag] = hex(sha)

    def _filter_for_bookmarks(self, bms):
        if not self.branch_bookmark_suffix:
            return [(bm, bm) for bm in bms]
        else:
            def _filter_bm(bm):
                if bm.endswith(self.branch_bookmark_suffix):
                    return bm[0:-(len(self.branch_bookmark_suffix))]
                else:
                    return bm
            return [(_filter_bm(bm), bm) for bm in bms]

    def local_heads(self):
        try:
            if getattr(bookmarks, 'parse', None):
                bms = bookmarks.parse(self.repo)
            else:
                bms = self.repo._bookmarks
            return dict([(filtered_bm, hex(bms[bm])) for
                        filtered_bm, bm in self._filter_for_bookmarks(bms)])
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
                        (obj_type, obj_sha) = obj.object
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

            # if there's a branch bookmark suffix,
            # then add it on to all bookmark names
            # that would otherwise conflict with a branch
            # name
            if self.branch_bookmark_suffix:
                real_branch_names = self.repo.branchmap()
                bms = dict(
                    (
                        bm_name + self.branch_bookmark_suffix
                            if bm_name in real_branch_names
                        else bm_name,
                        bms[bm_name]
                    )
                    for bm_name in bms
                )
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
        tagfile = self.repo.join(os.path.join('git-remote-refs'))
        tags = self.repo.gitrefs()
        # since we re-write all refs for this remote each time, prune
        # all entries matching this remote from our tags list now so
        # that we avoid any stale refs hanging around forever
        for t in list(tags):
            if t.startswith(remote_name + '/'):
                del tags[t]
        tags = dict((k, hex(v)) for k, v in tags.iteritems())
        store = self.git.object_store
        for ref_name, sha in refs.iteritems():
            if ref_name.startswith('refs/heads'):
                if sha not in store:
                    continue
                hgsha = self.map_hg_get(sha)
                head = ref_name[11:]
                tags['/'.join((remote_name, head))] = hgsha
                # TODO(durin42): what is this doing?
                new_ref = 'refs/remotes/%s/%s' % (remote_name, head)
                self.git.refs[new_ref] = sha
            elif (ref_name.startswith('refs/tags')
                  and not ref_name.endswith('^{}')):
                self.git.refs[ref_name] = sha

        tf = open(tagfile, 'wb')
        for tag, node in tags.iteritems():
            tf.write('%s %s\n' % (node, tag))
        tf.close()


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

                if ' : ' not in line:
                    break
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

    def collect_gitlinks(self, tree_id):
        """Walk the tree and collect all gitlink entries
          :param tree_id: sha of the commit tree
          :return: list of tuples (commit sha, full entry path)
        """
        queue = [(tree_id, '')]
        gitlinks = []
        while queue:
            e, path = queue.pop(0)
            o = self.git.object_store[e]
            for (name, mode, sha) in o.iteritems():
                if mode == S_IFGITLINK:
                    gitlinks.append((posixpath.join(path, name), sha))
                elif stat.S_ISDIR(mode):
                    queue.append((sha, posixpath.join(path, name)))
        return gitlinks

    def parse_gitmodules(self, tree_obj):
        """Parse .gitmodules from a git tree specified by tree_obj

           :return: list of tuples (submodule path, url, name),
           where name is quoted part of the section's name, or
           empty list if nothing found
        """
        rv = []
        try:
            unused_mode,gitmodules_sha = tree_obj['.gitmodules']
        except KeyError:
            return rv
        gitmodules_content = self.git[gitmodules_sha].data
        fo = StringIO.StringIO(gitmodules_content)
        tt = dul_config.ConfigFile.from_file(fo)
        for section in tt.keys():
            section_kind, section_name = section
            if section_kind == 'submodule':
                sm_path = tt.get(section, 'path')
                sm_url  = tt.get(section, 'url')
                rv.append((sm_path, sm_url, section_name))
        return rv

    def git_file_readlines(self, tree_obj, fname):
        """Read content of a named entry from the git commit tree

           :return: list of lines
        """
        if fname in tree_obj:
            unused_mode, sha = tree_obj[fname]
            content = self.git[sha].data
            return content.splitlines()
        return []

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
        # pass hg's ui.ssh config to dulwich
        if not issubclass(client.get_ssh_vendor, _ssh.SSHVendor):
            client.get_ssh_vendor = _ssh.generate_ssh_vendor(self.ui)

        # Test for git:// and git+ssh:// URI.
        #  Support several URL forms, including separating the
        #  host and path with either a / or : (sepr)
        git_pattern = re.compile(
            r'^(?P<scheme>git([+]ssh)?://)(?P<host>.*?)(:(?P<port>\d+))?'
            r'(?P<sepr>[:/])(?P<path>.*)$'
        )
        git_match = git_pattern.match(uri)
        if git_match:
            res = git_match.groupdict()
            transport = client.SSHGitClient if 'ssh' in res['scheme'] else client.TCPGitClient
            host, port, sepr, path = res['host'], res['port'], res['sepr'], res['path']
            if sepr == '/':
                path = '/' + path
            # strip trailing slash for heroku-style URLs
            # ssh+git://git@heroku.com:project.git/
            if sepr == ':' and path.endswith('.git/'):
                path = path.rstrip('/')
            if port:
                client.port = port

            return transport(host, thin_packs=False, port=port), path

        httpclient = getattr(client, 'HttpGitClient', None)

        if uri.startswith('git+http://') or uri.startswith('git+https://'):
            uri = uri[4:]

        if uri.startswith('http://') or uri.startswith('https://'):
            if not httpclient:
                raise RepoError('git via HTTP requires dulwich 0.8.1 or later')
            else:
                return client.HttpGitClient(uri, thin_packs=False), uri

        # if its not git or git+ssh, try a local url..
        return client.SubprocessGitClient(thin_packs=False), uri
