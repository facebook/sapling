import os, math, urllib, urllib2, re
import stat, posixpath, StringIO

from dulwich.errors import HangupException, GitProtocolError, UpdateRefsError
from dulwich.objects import Blob, Commit, Tag, Tree, parse_timezone, S_IFGITLINK
from dulwich.pack import create_delta, apply_delta
from dulwich.repo import Repo, check_ref_format
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
from mercurial import url

import _ssh
import hg2git
import util
from overlay import overlayrepo


RE_GIT_AUTHOR = re.compile('^(.*?) ?\<(.*?)(?:\>(.*))?$')

RE_GIT_SANITIZE_AUTHOR = re.compile('[<>\n]')

RE_GIT_AUTHOR_EXTRA = re.compile('^(.*?)\ ext:\((.*)\) <(.*)\>$')

# Test for git:// and git+ssh:// URI.
# Support several URL forms, including separating the
# host and path with either a / or : (sepr)
RE_GIT_URI = re.compile(
    r'^(?P<scheme>git([+]ssh)?://)(?P<host>.*?)(:(?P<port>\d+))?'
    r'(?P<sepr>[:/])(?P<path>.*)$')

RE_NEWLINES = re.compile('[\r\n]')
RE_GIT_PROGRESS = re.compile('\((\d+)/(\d+)\)')

RE_AUTHOR_FILE = re.compile('\s*=\s*')

class GitProgress(object):
    """convert git server progress strings into mercurial progress"""
    def __init__(self, ui):
        self.ui = ui

        self.lasttopic = None
        self.msgbuf = ''

    def progress(self, msg):
        # 'Counting objects: 33640, done.\n'
        # 'Compressing objects:   0% (1/9955)   \r
        msgs = RE_NEWLINES.split(self.msgbuf + msg)
        self.msgbuf = msgs.pop()

        for msg in msgs:
            td = msg.split(':', 1)
            data = td.pop()
            if not td:
                self.flush(data)
                continue
            topic = td[0]

            m = RE_GIT_PROGRESS.search(data)
            if m:
                if self.lasttopic and self.lasttopic != topic:
                    self.flush()
                self.lasttopic = topic

                pos, total = map(int, m.group(1, 2))
                self.ui.progress(topic, pos, total=total)
            else:
                self.flush(msg)

    def flush(self, msg=None):
        if self.lasttopic:
            self.ui.progress(self.lasttopic, None)
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

        self._map_git_real = {}
        self._map_hg_real = {}
        self.load_tags()

    @property
    def _map_git(self):
      if not self._map_git_real:
        self.load_map()
      return self._map_git_real

    @property
    def _map_hg(self):
      if not self._map_hg_real:
        self.load_map()
      return self._map_hg_real

    @hgutil.propertycache
    def git(self):
        # make the git data directory
        if os.path.exists(self.gitdir):
            return Repo(self.gitdir)
        else:
            os.mkdir(self.gitdir)
            return Repo.init_bare(self.gitdir)

    def init_author_file(self):
        self.author_map = {}
        if self.ui.config('git', 'authors'):
            with open(self.repo.wjoin(self.ui.config('git', 'authors'))) as f:
                for line in f:
                    line = line.strip()
                    if not line or line.startswith('#'):
                        continue
                    from_, to = RE_AUTHOR_FILE.split(line, 2)
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
        if os.path.exists(self.repo.join(self.mapfile)):
            for line in self.repo.opener(self.mapfile):
                gitsha, hgsha = line.strip().split(' ', 1)
                self._map_git_real[gitsha] = hgsha
                self._map_hg_real[hgsha] = gitsha

    def save_map(self):
        file = self.repo.opener(self.mapfile, 'w+', atomictemp=True)
        for hgsha, gitsha in sorted(self._map_hg.iteritems()):
            file.write("%s %s\n" % (gitsha, hgsha))
        # If this complains, atomictempfile no longer has close
        file.close()

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
        # If this complains, atomictempfile no longer has close
        file.close()

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
        oldheads = self.repo.changelog.heads()
        imported = 0
        if refs:
            filteredrefs = self.filter_refs(refs, heads)
            imported = self.import_git_objects(remote_name, filteredrefs)
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

        self.save_map()

        if imported == 0:
            return 0

        # code taken from localrepo.py:addchangegroup
        dh = 0
        if oldheads:
            heads = self.repo.changelog.heads()
            dh = len(heads) - len(oldheads)
            for h in heads:
                if h not in oldheads and self.repo[h].closesbranch():
                    dh -= 1

        if dh < 0:
            return dh - 1
        else:
            return dh + 1

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
            return refs # always return the same refs to make the send a no-op

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
        old_refs, new_refs = self.upload_pack(remote, revs, force)
        remote_name = self.remote_name(remote)

        if remote_name and new_refs:
            for ref, new_sha in sorted(new_refs.iteritems()):
                old_sha = old_refs.get(ref)
                if old_sha is None:
                    if self.ui.verbose:
                        self.ui.note("adding reference %s::%s => GIT:%s\n" %
                                   (remote_name, ref, new_sha[0:8]))
                    else:
                        self.ui.status("adding reference %s\n" % ref)
                elif new_sha != old_sha:
                    if self.ui.verbose:
                        self.ui.note("updating reference %s::%s => GIT:%s\n" %
                                   (remote_name, ref, new_sha[0:8]))
                    else:
                        self.ui.status("updating reference %s\n" % ref)
                else:
                    self.ui.debug("unchanged reference %s::%s => GIT:%s\n" %
                                   (remote_name, ref, new_sha[0:8]))

            self.update_remote_branches(remote_name, new_refs)
        if old_refs == new_refs:
            self.ui.status(_("no changes found\n"))
            ret = None
        elif len(new_refs) > len(old_refs):
            ret = 1 + (len(new_refs) - len(old_refs))
        elif len(old_refs) > len(new_refs):
            ret = -1 - (len(new_refs) - len(old_refs))
        else:
            ret = 1
        return ret

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
        clnode = self.repo.changelog.node
        nodes = [clnode(n) for n in self.repo]
        export = [node for node in nodes if not hex(node) in self._map_hg]
        total = len(export)
        if not total:
            return

        self.ui.note(_("exporting hg objects to git\n"))

        # By only exporting deltas, the assertion is that all previous objects
        # for all other changesets are already present in the Git repository.
        # This assertion is necessary to prevent redundant work. Here, nodes,
        # and therefore export, is in topological order. By definition,
        # export[0]'s parents must be present in Git, so we start the
        # incremental exporter from there.
        pctx = self.repo[export[0]].p1()
        pnode = pctx.node()
        if pnode == nullid:
            gitcommit = None
        else:
            gitsha = self._map_hg[hex(pnode)]
            try:
                gitcommit = self.git[gitsha]
            except KeyError:
                raise hgutil.Abort(_('Parent SHA-1 not present in Git'
                                     'repo: %s' % gitsha))

        exporter = hg2git.IncrementalChangesetExporter(
            self.repo, pctx, self.git.object_store, gitcommit)

        for i, rev in enumerate(export):
            self.ui.progress('exporting', i, total=total)
            ctx = self.repo.changectx(rev)
            state = ctx.extra().get('hg-git', None)
            if state == 'octopus':
                self.ui.debug("revision %d is a part "
                              "of octopus explosion\n" % ctx.rev())
                continue
            self.export_hg_commit(rev, exporter)
        self.ui.progress('exporting', None, total=total)


    # convert this commit into git objects
    # go through the manifest, convert all blobs/trees we don't have
    # write the commit object (with metadata info)
    def export_hg_commit(self, rev, exporter):
        self.ui.note(_("converting revision %s\n") % hex(rev))

        oldenc = self.swap_out_encoding()

        ctx = self.repo.changectx(rev)
        extra = ctx.extra()

        commit = Commit()

        (time, timezone) = ctx.date()
        # work around to bad timezone offets - dulwich does not handle
        # sub minute based timezones. In the one known case, it was a
        # manual edit that led to the unusual value. Based on that,
        # there is no reason to round one way or the other, so do the
        # simplest and round down.
        timezone -= (timezone % 60)
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
                if git_sha not in self.git.object_store:
                    raise hgutil.Abort(_('Parent SHA-1 not present in Git'
                                         'repo: %s' % git_sha))

                commit.parents.append(git_sha)

        commit.message = self.get_git_message(ctx)

        if 'encoding' in extra:
            commit.encoding = extra['encoding']

        for obj, nodeid in exporter.update_changeset(ctx):
            if obj.id not in self.git.object_store:
                self.git.object_store.add_object(obj)

        tree_sha = exporter.root_tree_sha

        if tree_sha not in self.git.object_store:
            raise hgutil.Abort(_('Tree SHA-1 not present in Git repo: %s' %
                tree_sha))

        commit.tree = tree_sha

        if commit.id not in self.git.object_store:
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
        return RE_GIT_SANITIZE_AUTHOR.sub('?', name.lstrip('< ').rstrip('> '))

    def get_git_author(self, ctx):
        # hg authors might not have emails
        author = ctx.user()

        # see if a translation exists
        author = self.author_map.get(author, author)

        # check for git author pattern compliance
        a = RE_GIT_AUTHOR.match(author)

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
                assert ctx.extra().get('hg-git', None) != 'octopus'
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

    def getnewgitcommits(self, refs=None):
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

        # traverse the heads getting a list of all the unique commits in
        # topological order
        commits = []
        seen = set(todo)
        while todo:
            sha = todo[-1]
            if sha in done or sha in self._map_git:
                todo.pop()
                continue
            assert isinstance(sha, str)
            if sha in convert_list:
                obj = convert_list[sha]
            else:
                obj = self.git.get_object(sha)
                convert_list[sha] = obj
            assert isinstance(obj, Commit)
            for p in obj.parents:
                if p not in done and p not in self._map_git:
                    todo.append(p)
                    # process parents of a commit before processing the
                    # commit itself, and come back to this commit later
                    break
            else:
                commits.append(sha)
                done.add(sha)
                todo.pop()

        return convert_list, commits

    def import_git_objects(self, remote_name=None, refs=None):
        convert_list, commits = self.getnewgitcommits(refs)
        # import each of the commits, oldest first
        total = len(commits)
        if total:
            self.ui.status(_("importing git objects into hg\n"))
        else:
            self.ui.status(_("no changes found\n"))

        for i, csha in enumerate(commits):
            self.ui.progress('importing', i, total=total, unit='commits')
            commit = convert_list[csha]
            self.import_git_commit(commit)
        self.ui.progress('importing', None, total=total, unit='commits')

        # TODO if the tags cache is used, remove any dangling tag references
        return total

    def import_git_commit(self, commit):
        self.ui.debug(_("importing: %s\n") % commit.id)

        (strip_message, hg_renames,
         hg_branch, extra) = self.extract_hg_metadata(commit.message)

        gparents = map(self.map_hg_get, commit.parents)

        for parent in gparents:
            if parent not in self.repo:
                raise hgutil.Abort(_('you appear to have run strip - '
                                     'please run hg git-cleanup'))

        # get a list of the changed, added, removed files and gitlinks
        files, gitlinks = self.get_files_changed(commit)

        git_commit_tree = self.git[commit.tree]

        # Analyze hgsubstate and build an updated version using SHAs from
        # gitlinks. Order of application:
        # - preexisting .hgsubstate in git tree
        # - .hgsubstate from hg parent
        # - changes in gitlinks
        hgsubstate = util.parse_hgsubstate(
            self.git_file_readlines(git_commit_tree, '.hgsubstate'))
        parentsubdata = ''
        if gparents:
            p1ctx = self.repo.changectx(gparents[0])
            if '.hgsubstate' in p1ctx:
                parentsubdata = p1ctx.filectx('.hgsubstate').data().splitlines()
                parentsubstate = util.parse_hgsubstate(parentsubdata)
                for path, sha in parentsubstate.iteritems():
                    hgsubstate[path] = sha
        for path, sha in gitlinks.iteritems():
            if sha is None:
                hgsubstate.pop(path, None)
            else:
                hgsubstate[path] = sha
        # in case .hgsubstate wasn't among changed files
        # force its inclusion
        if not hgsubstate and parentsubdata:
            files['.hgsubstate'] = True, None, None
        elif util.serialize_hgsubstate(hgsubstate) != parentsubdata:
            files['.hgsubstate'] = False, 0100644, None

        # Analyze .hgsub and merge with .gitmodules
        hgsub = None
        gitmodules = self.parse_gitmodules(git_commit_tree)
        if gitmodules:
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
            m = RE_GIT_AUTHOR_EXTRA.match(commit.author)
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
                    if getattr(memctx, '_returnnoneformissingfiles', False):
                        return None
                    else:  # Mercurial < 3.2
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

            try:
                return context.memfilectx(self.repo, f, data,
                                          islink='l' in e,
                                          isexec='x' in e,
                                          copied=copied_path)
            except TypeError:
                return context.memfilectx(f, data,
                                          islink='l' in e,
                                          isexec='x' in e,
                                          copied=copied_path)

        p1, p2 = (nullid, nullid)
        octopus = False

        if len(gparents) > 1:
            # merge, possibly octopus
            def commit_octopus(p1, p2):
                ctx = context.memctx(self.repo, (p1, p2), text,
                                     list(files) + findconvergedfiles(p1, p2),
                                     getfilectx, author, date, {'hg-git': 'octopus'})
                # See comment below about setting substate to None.
                ctx.substate = None
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

        ctx = context.memctx(self.repo, (p1, p2), text,
                             list(files) + findconvergedfiles(p1, p2),
                             getfilectx, author, date, extra)
        # Starting Mercurial commit d2743be1bb06, memctx imports from
        # committablectx. This means that it has a 'substate' property that
        # contains the subrepo state. Ordinarily, Mercurial expects the subrepo
        # to be present while making a new commit -- since hg-git is importing
        # purely in-memory commits without backing stores for the subrepos, that
        # won't work. Forcibly set the substate to None so that there's no
        # attempt to read subrepos.
        ctx.substate = None
        node = self.repo.commitctx(ctx)

        self.swap_out_encoding(oldenc)

        # save changeset to mapping file
        cs = hex(node)
        self.map_set(commit.id, cs)

    ## PACK UPLOADING AND FETCHING

    def upload_pack(self, remote, revs, force):
        client, path = self.get_transport_and_path(remote)
        old_refs = {}
        change_totals = {}

        def changed(refs):
            self.ui.status(_("searching for changes\n"))
            old_refs.update(refs)
            to_push = revs or set(self.local_heads().values() + self.tags.values())
            return self.get_changed_refs(refs, to_push, force)

        def genpack(have, want):
            commits = []
            for mo in self.git.object_store.find_missing_objects(have, want):
                (sha, name) = mo
                o = self.git.object_store[sha]
                t = type(o)
                change_totals[t] = change_totals.get(t, 0) + 1
                if isinstance(o, Commit):
                    commits.append(sha)
            commit_count = len(commits)
            self.ui.note(_("%d commits found\n") % commit_count)
            if commit_count > 0:
                self.ui.debug(_("list of commits:\n"))
                for commit in commits:
                    self.ui.debug("%s\n" % commit)
                self.ui.status(_("adding objects\n"))
            return self.git.object_store.generate_pack_contents(have, want)

        try:
            new_refs = client.send_pack(path, changed, genpack)
            if len(change_totals) > 0:
                self.ui.status(_("added %d commits with %d trees"
                                 " and %d blobs\n") %
                               (change_totals.get(Commit, 0),
                                change_totals.get(Tree, 0),
                                change_totals.get(Blob, 0)))
            return old_refs, new_refs
        except (HangupException, GitProtocolError), e:
            raise hgutil.Abort(_("git remote error: ") + str(e))

    def get_changed_refs(self, refs, revs, force):
        new_refs = refs.copy()

        #The remote repo is empty and the local one doesn't have bookmarks/tags
        if refs.keys()[0] == 'capabilities^{}':
            if not self.local_heads():
                tip = self.repo.lookup('tip')
                if tip != nullid:
                    del new_refs['capabilities^{}']
                    tip = hex(tip)
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
                    raise hgutil.Abort(
                        "branch '%s' changed on the server, "
                        "please pull and merge before pushing" % ref)

        return new_refs

    def fetch_pack(self, remote_name, heads=None):
        client, path = self.get_transport_and_path(remote_name)
        graphwalker = self.git.get_graph_walker()

        def determine_wants(refs):
            filteredrefs = self.filter_refs(refs, heads)
            return [x for x in filteredrefs.itervalues() if x not in self.git]

        try:
            progress = GitProgress(self.ui)
            f = StringIO.StringIO()
            ret = client.fetch_pack(path, determine_wants, graphwalker, f.write, progress.progress)
            if(f.pos != 0):
                f.seek(0)
                po =  self.git.object_store.add_thin_pack(f.read, None)
            progress.flush()

            # For empty repos dulwich gives us None, but since later
            # we want to iterate over this, we really want an empty
            # iterable
            return ret if ret else {}
        except (HangupException, GitProtocolError), e:
            raise hgutil.Abort(_("git remote error: ") + str(e))

    ## REFERENCES HANDLING

    def filter_refs(self, refs, heads):
        '''For a dictionary of refs: shas, if heads is None then return refs
        that match the heads. Otherwise, return refs that are heads or tags.

        '''
        filteredrefs = {}
        if heads is not None:
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
                    filteredrefs[r[0]] = refs[r[0]]
                else:
                    raise hgutil.Abort("ambiguous reference %s: %r" % (h, r))
        else:
            for ref, sha in refs.iteritems():
                if (not ref.endswith('^{}')
                    and (ref.startswith('refs/heads/')
                         or ref.startswith('refs/tags/'))):
                    filteredrefs[ref] = sha
        return filteredrefs

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
                target = self.map_git_get(hex(sha))
                if target is not None:
                    tag_refname = 'refs/tags/' + tag
                    if(check_ref_format(tag_refname)):
                      self.git.refs[tag_refname] = target
                      self.tags[tag] = hex(sha)
                    else:
                      self.repo.ui.warn(
                        'Skipping export of tag %s because it '
                        'has invalid name as a git refname.\n' % tag)
                else:
                    self.repo.ui.warn(
                        'Skipping export of tag %s because it '
                        'has no matching git revision.\n' % tag)

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
        repotags = self.repo.tags()
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
                if not ref_name in repotags:
                    obj = self.git.get_object(refs[k])
                    sha = None
                    if isinstance (obj, Commit): # lightweight
                        sha = self.map_hg_get(refs[k])
                        if sha is not None:
                            self.tags[ref_name] = sha
                    elif isinstance (obj, Tag): # annotated
                        (obj_type, obj_sha) = obj.object
                        obj = self.git.get_object(obj_sha)
                        if isinstance (obj, Commit):
                            sha = self.map_hg_get(obj_sha)
                            # TODO: better handling for annotated tags
                            if sha is not None:
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

            suffix = self.branch_bookmark_suffix or ''
            for head, sha in heads.iteritems():
                # refs contains all the refs in the server, not just
                # the ones we are pulling
                hgsha = self.map_hg_get(sha)
                if hgsha is None:
                    continue
                hgsha = bin(hgsha)
                if not head in bms:
                    # new branch
                    bms[head + suffix] = hgsha
                else:
                    bm = self.repo[bms[head]]
                    if bm.ancestor(self.repo[hgsha]) == bm:
                        # fast forward
                        bms[head + suffix] = hgsha

            if heads:
                if oldbm:
                    bookmarks.write(self.repo, bms)
                else:
                    self.repo._bookmarks = bms
                    if getattr(bms, 'write', None): # hg >= 2.5
                        bms.write()
                    else: # hg < 2.5
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
                hgsha = self.map_hg_get(sha)
                if hgsha is None or hgsha not in self.repo:
                    continue
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
        gitlinks = {}
        for (oldfile, newfile), (oldmode, newmode), (oldsha, newsha) in changes:
            # actions are described by the following table ('no' means 'does not
            # exist'):
            #    old        new     |    action
            #     no        file    |  record file
            #     no      gitlink   |  record gitlink
            #    file        no     |  delete file
            #    file       file    |  record file
            #    file     gitlink   |  delete file and record gitlink
            #  gitlink       no     |  delete gitlink
            #  gitlink      file    |  delete gitlink and record file
            #  gitlink    gitlink   |  record gitlink
            if newmode == 0160000:
                # new = gitlink
                gitlinks[newfile] = newsha
                if oldmode is not None and oldmode != 0160000:
                    # file -> gitlink
                    files[oldfile] = True, None, None
                continue
            if oldmode == 0160000 and newmode != 0160000:
                # gitlink -> no/file (gitlink -> gitlink is covered above)
                gitlinks[oldfile] = None
                continue
            if newfile is not None:
                # new = file
                files[newfile] = False, newmode, newsha
            else:
                # old = file
                files[oldfile] = True, None, None

        return files, gitlinks

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

        git_match = RE_GIT_URI.match(uri)
        if git_match:
            res = git_match.groupdict()
            transport = client.SSHGitClient if 'ssh' in res['scheme'] else client.TCPGitClient
            host, port, sepr, path = res['host'], res['port'], res['sepr'], res['path']
            if sepr == '/' and not path.startswith('~'):
                path = '/' + path
            # strip trailing slash for heroku-style URLs
            # ssh+git://git@heroku.com:project.git/
            if sepr == ':' and path.endswith('.git/'):
                path = path.rstrip('/')
            if port:
                client.port = port

            return transport(host, port=port), path

        if uri.startswith('git+http://') or uri.startswith('git+https://'):
            uri = uri[4:]

        if uri.startswith('http://') or uri.startswith('https://'):
            auth_handler = urllib2.HTTPBasicAuthHandler(url.passwordmgr(self.ui))
            opener = urllib2.build_opener(auth_handler)
            useragent = 'git/20x6 (hg-git ; uses dulwich and hg ; like git-core)'
            opener.addheaders = [('User-Agent', useragent)]
            return client.HttpGitClient(uri, opener=opener), uri

        # if its not git or git+ssh, try a local url..
        return client.SubprocessGitClient(), uri
