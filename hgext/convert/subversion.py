# Subversion 1.4/1.5 Python API backend
#
# Copyright(C) 2007 Daniel Holth et al
#
# Configuration options:
#
# convert.svn.trunk
#   Relative path to the trunk (default: "trunk")
# convert.svn.branches
#   Relative path to tree of branches (default: "branches")
# convert.svn.tags
#   Relative path to tree of tags (default: "tags")
#
# Set these in a hgrc, or on the command line as follows:
#
#   hg convert --config convert.svn.trunk=wackoname [...]

import locale
import os
import re
import sys
import cPickle as pickle
import tempfile

from mercurial import strutil, util
from mercurial.i18n import _

# Subversion stuff. Works best with very recent Python SVN bindings
# e.g. SVN 1.5 or backports. Thanks to the bzr folks for enhancing
# these bindings.

from cStringIO import StringIO

from common import NoRepo, commit, converter_source, encodeargs, decodeargs
from common import commandline, converter_sink, mapfile

try:
    from svn.core import SubversionException, Pool
    import svn
    import svn.client
    import svn.core
    import svn.ra
    import svn.delta
    import transport
except ImportError:
    pass

def geturl(path):
    try:
        return svn.client.url_from_path(svn.core.svn_path_canonicalize(path))
    except SubversionException:
        pass
    if os.path.isdir(path):
        path = os.path.normpath(os.path.abspath(path))
        if os.name == 'nt':
            path = '/' + util.normpath(path)
        return 'file://%s' % path
    return path

def optrev(number):
    optrev = svn.core.svn_opt_revision_t()
    optrev.kind = svn.core.svn_opt_revision_number
    optrev.value.number = number
    return optrev

class changedpath(object):
    def __init__(self, p):
        self.copyfrom_path = p.copyfrom_path
        self.copyfrom_rev = p.copyfrom_rev
        self.action = p.action

def get_log_child(fp, url, paths, start, end, limit=0, discover_changed_paths=True,
                    strict_node_history=False):
    protocol = -1
    def receiver(orig_paths, revnum, author, date, message, pool):
        if orig_paths is not None:
            for k, v in orig_paths.iteritems():
                orig_paths[k] = changedpath(v)
        pickle.dump((orig_paths, revnum, author, date, message),
                    fp, protocol)

    try:
        # Use an ra of our own so that our parent can consume
        # our results without confusing the server.
        t = transport.SvnRaTransport(url=url)
        svn.ra.get_log(t.ra, paths, start, end, limit,
                       discover_changed_paths,
                       strict_node_history,
                       receiver)
    except SubversionException, (inst, num):
        pickle.dump(num, fp, protocol)
    except IOError:
        # Caller may interrupt the iteration
        pickle.dump(None, fp, protocol)
    else:
        pickle.dump(None, fp, protocol)
    fp.close()
    # With large history, cleanup process goes crazy and suddenly
    # consumes *huge* amount of memory. The output file being closed,
    # there is no need for clean termination.
    os._exit(0)

def debugsvnlog(ui, **opts):
    """Fetch SVN log in a subprocess and channel them back to parent to
    avoid memory collection issues.
    """
    util.set_binary(sys.stdin)
    util.set_binary(sys.stdout)
    args = decodeargs(sys.stdin.read())
    get_log_child(sys.stdout, *args)

class logstream:
    """Interruptible revision log iterator."""
    def __init__(self, stdout):
        self._stdout = stdout

    def __iter__(self):
        while True:
            entry = pickle.load(self._stdout)
            try:
                orig_paths, revnum, author, date, message = entry
            except:
                if entry is None:
                    break
                raise SubversionException("child raised exception", entry)
            yield entry

    def close(self):
        if self._stdout:
            self._stdout.close()
            self._stdout = None

# SVN conversion code stolen from bzr-svn and tailor
#
# Subversion looks like a versioned filesystem, branches structures
# are defined by conventions and not enforced by the tool. First,
# we define the potential branches (modules) as "trunk" and "branches"
# children directories. Revisions are then identified by their
# module and revision number (and a repository identifier).
#
# The revision graph is really a tree (or a forest). By default, a
# revision parent is the previous revision in the same module. If the
# module directory is copied/moved from another module then the
# revision is the module root and its parent the source revision in
# the parent module. A revision has at most one parent.
#
class svn_source(converter_source):
    def __init__(self, ui, url, rev=None):
        super(svn_source, self).__init__(ui, url, rev=rev)

        try:
            SubversionException
        except NameError:
            raise NoRepo('Subversion python bindings could not be loaded')

        self.encoding = locale.getpreferredencoding()
        self.lastrevs = {}

        latest = None
        try:
            # Support file://path@rev syntax. Useful e.g. to convert
            # deleted branches.
            at = url.rfind('@')
            if at >= 0:
                latest = int(url[at+1:])
                url = url[:at]
        except ValueError, e:
            pass
        self.url = geturl(url)
        self.encoding = 'UTF-8' # Subversion is always nominal UTF-8
        try:
            self.transport = transport.SvnRaTransport(url=self.url)
            self.ra = self.transport.ra
            self.ctx = self.transport.client
            self.base = svn.ra.get_repos_root(self.ra)
            # Module is either empty or a repository path starting with
            # a slash and not ending with a slash.
            self.module = self.url[len(self.base):]
            self.prevmodule = None
            self.rootmodule = self.module
            self.commits = {}
            self.paths = {}
            self.uuid = svn.ra.get_uuid(self.ra).decode(self.encoding)
        except SubversionException, e:
            ui.print_exc()
            raise NoRepo("%s does not look like a Subversion repo" % self.url)

        if rev:
            try:
                latest = int(rev)
            except ValueError:
                raise util.Abort('svn: revision %s is not an integer' % rev)

        self.startrev = self.ui.config('convert', 'svn.startrev', default=0)
        try:
            self.startrev = int(self.startrev)
            if self.startrev < 0:
                self.startrev = 0
        except ValueError:
            raise util.Abort(_('svn: start revision %s is not an integer')
                             % self.startrev)

        try:
            self.get_blacklist()
        except IOError, e:
            pass

        self.head = self.latest(self.module, latest)
        if not self.head:
            raise util.Abort(_('no revision found in module %s') %
                             self.module.encode(self.encoding))
        self.last_changed = self.revnum(self.head)

        self._changescache = None

        if os.path.exists(os.path.join(url, '.svn/entries')):
            self.wc = url
        else:
            self.wc = None
        self.convertfp = None

    def setrevmap(self, revmap):
        lastrevs = {}
        for revid in revmap.iterkeys():
            uuid, module, revnum = self.revsplit(revid)
            lastrevnum = lastrevs.setdefault(module, revnum)
            if revnum > lastrevnum:
                lastrevs[module] = revnum
        self.lastrevs = lastrevs

    def exists(self, path, optrev):
        try:
            svn.client.ls(self.url.rstrip('/') + '/' + path,
                                 optrev, False, self.ctx)
            return True
        except SubversionException, err:
            return False

    def getheads(self):

        def isdir(path, revnum):
            kind = self._checkpath(path, revnum)
            return kind == svn.core.svn_node_dir

        def getcfgpath(name, rev):
            cfgpath = self.ui.config('convert', 'svn.' + name)
            if cfgpath is not None and cfgpath.strip() == '':
                return None
            path = (cfgpath or name).strip('/')
            if not self.exists(path, rev):
                if cfgpath:
                    raise util.Abort(_('expected %s to be at %r, but not found')
                                 % (name, path))
                return None
            self.ui.note(_('found %s at %r\n') % (name, path))
            return path

        rev = optrev(self.last_changed)
        oldmodule = ''
        trunk = getcfgpath('trunk', rev)
        self.tags = getcfgpath('tags', rev)
        branches = getcfgpath('branches', rev)

        # If the project has a trunk or branches, we will extract heads
        # from them. We keep the project root otherwise.
        if trunk:
            oldmodule = self.module or ''
            self.module += '/' + trunk
            self.head = self.latest(self.module, self.last_changed)
            if not self.head:
                raise util.Abort(_('no revision found in module %s') %
                                 self.module.encode(self.encoding))

        # First head in the list is the module's head
        self.heads = [self.head]
        if self.tags is not None:
            self.tags = '%s/%s' % (oldmodule , (self.tags or 'tags'))

        # Check if branches bring a few more heads to the list
        if branches:
            rpath = self.url.strip('/')
            branchnames = svn.client.ls(rpath + '/' + branches, rev, False,
                                        self.ctx)
            for branch in branchnames.keys():
                module = '%s/%s/%s' % (oldmodule, branches, branch)
                if not isdir(module, self.last_changed):
                    continue
                brevid = self.latest(module, self.last_changed)
                if not brevid:
                    self.ui.note(_('ignoring empty branch %s\n') %
                                   branch.encode(self.encoding))
                    continue
                self.ui.note('found branch %s at %d\n' %
                             (branch, self.revnum(brevid)))
                self.heads.append(brevid)

        if self.startrev and self.heads:
            if len(self.heads) > 1:
                raise util.Abort(_('svn: start revision is not supported with '
                                   'with more than one branch'))
            revnum = self.revnum(self.heads[0])
            if revnum < self.startrev:
                raise util.Abort(_('svn: no revision found after start revision %d')
                                 % self.startrev)

        return self.heads

    def getfile(self, file, rev):
        data, mode = self._getfile(file, rev)
        self.modecache[(file, rev)] = mode
        return data

    def getmode(self, file, rev):
        return self.modecache[(file, rev)]

    def getchanges(self, rev):
        if self._changescache and self._changescache[0] == rev:
            return self._changescache[1]
        self._changescache = None
        self.modecache = {}
        (paths, parents) = self.paths[rev]
        if parents:
            files, copies = self.expandpaths(rev, paths, parents)
        else:
            # Perform a full checkout on roots
            uuid, module, revnum = self.revsplit(rev)
            entries = svn.client.ls(self.base + module, optrev(revnum),
                                    True, self.ctx)
            files = [n for n,e in entries.iteritems()
                     if e.kind == svn.core.svn_node_file]
            copies = {}

        files.sort()
        files = zip(files, [rev] * len(files))

        # caller caches the result, so free it here to release memory
        del self.paths[rev]
        return (files, copies)

    def getchangedfiles(self, rev, i):
        changes = self.getchanges(rev)
        self._changescache = (rev, changes)
        return [f[0] for f in changes[0]]

    def getcommit(self, rev):
        if rev not in self.commits:
            uuid, module, revnum = self.revsplit(rev)
            self.module = module
            self.reparent(module)
            # We assume that:
            # - requests for revisions after "stop" come from the
            # revision graph backward traversal. Cache all of them
            # down to stop, they will be used eventually.
            # - requests for revisions before "stop" come to get
            # isolated branches parents. Just fetch what is needed.
            stop = self.lastrevs.get(module, 0)
            if revnum < stop:
                stop = revnum + 1
            self._fetch_revisions(revnum, stop)
        commit = self.commits[rev]
        # caller caches the result, so free it here to release memory
        del self.commits[rev]
        return commit

    def gettags(self):
        tags = {}
        if self.tags is None:
            return tags

        # svn tags are just a convention, project branches left in a
        # 'tags' directory. There is no other relationship than
        # ancestry, which is expensive to discover and makes them hard
        # to update incrementally.  Worse, past revisions may be
        # referenced by tags far away in the future, requiring a deep
        # history traversal on every calculation.  Current code
        # performs a single backward traversal, tracking moves within
        # the tags directory (tag renaming) and recording a new tag
        # everytime a project is copied from outside the tags
        # directory. It also lists deleted tags, this behaviour may
        # change in the future.
        pendings = []
        tagspath = self.tags
        start = svn.ra.get_latest_revnum(self.ra)
        try:
            for entry in self._getlog([self.tags], start, self.startrev):
                origpaths, revnum, author, date, message = entry
                copies = [(e.copyfrom_path, e.copyfrom_rev, p) for p, e
                          in origpaths.iteritems() if e.copyfrom_path]
                copies.sort()
                # Apply moves/copies from more specific to general
                copies.reverse()

                srctagspath = tagspath
                if copies and copies[-1][2] == tagspath:
                    # Track tags directory moves
                    srctagspath = copies.pop()[0]

                for source, sourcerev, dest in copies:
                    if not dest.startswith(tagspath + '/'):
                        continue
                    for tag in pendings:
                        if tag[0].startswith(dest):
                            tagpath = source + tag[0][len(dest):]
                            tag[:2] = [tagpath, sourcerev]
                            break
                    else:
                        pendings.append([source, sourcerev, dest.split('/')[-1]])

                # Tell tag renamings from tag creations
                remainings = []
                for source, sourcerev, tagname in pendings:
                    if source.startswith(srctagspath):
                        remainings.append([source, sourcerev, tagname])
                        continue
                    # From revision may be fake, get one with changes
                    tagid = self.latest(source, sourcerev)
                    if tagid:
                        tags[tagname] = tagid
                pendings = remainings
                tagspath = srctagspath

        except SubversionException, (inst, num):
            self.ui.note('no tags found at revision %d\n' % start)
        return tags

    def converted(self, rev, destrev):
        if not self.wc:
            return
        if self.convertfp is None:
            self.convertfp = open(os.path.join(self.wc, '.svn', 'hg-shamap'),
                                  'a')
        self.convertfp.write('%s %d\n' % (destrev, self.revnum(rev)))
        self.convertfp.flush()

    # -- helper functions --

    def revid(self, revnum, module=None):
        if not module:
            module = self.module
        return u"svn:%s%s@%s" % (self.uuid, module.decode(self.encoding),
                                 revnum)

    def revnum(self, rev):
        return int(rev.split('@')[-1])

    def revsplit(self, rev):
        url, revnum = rev.encode(self.encoding).split('@', 1)
        revnum = int(revnum)
        parts = url.split('/', 1)
        uuid = parts.pop(0)[4:]
        mod = ''
        if parts:
            mod = '/' + parts[0]
        return uuid, mod, revnum

    def latest(self, path, stop=0):
        """Find the latest revid affecting path, up to stop. It may return
        a revision in a different module, since a branch may be moved without
        a change being reported. Return None if computed module does not
        belong to rootmodule subtree.
        """
        if not path.startswith(self.rootmodule):
            # Requests on foreign branches may be forbidden at server level
            self.ui.debug(_('ignoring foreign branch %r\n') % path)
            return None

        if not stop:
            stop = svn.ra.get_latest_revnum(self.ra)
        try:
            prevmodule = self.reparent('')
            dirent = svn.ra.stat(self.ra, path.strip('/'), stop)
            self.reparent(prevmodule)
        except SubversionException:
            dirent = None
        if not dirent:
            raise util.Abort('%s not found up to revision %d' % (path, stop))

        # stat() gives us the previous revision on this line of development, but
        # it might be in *another module*. Fetch the log and detect renames down
        # to the latest revision.
        stream = self._getlog([path], stop, dirent.created_rev)
        try:
            for entry in stream:
                paths, revnum, author, date, message = entry
                if revnum <= dirent.created_rev:
                    break

                for p in paths:
                    if not path.startswith(p) or not paths[p].copyfrom_path:
                        continue
                    newpath = paths[p].copyfrom_path + path[len(p):]
                    self.ui.debug("branch renamed from %s to %s at %d\n" %
                                  (path, newpath, revnum))
                    path = newpath
                    break
        finally:
            stream.close()

        if not path.startswith(self.rootmodule):
            self.ui.debug(_('ignoring foreign branch %r\n') % path)
            return None
        return self.revid(dirent.created_rev, path)

    def get_blacklist(self):
        """Avoid certain revision numbers.
        It is not uncommon for two nearby revisions to cancel each other
        out, e.g. 'I copied trunk into a subdirectory of itself instead
        of making a branch'. The converted repository is significantly
        smaller if we ignore such revisions."""
        self.blacklist = util.set()
        blacklist = self.blacklist
        for line in file("blacklist.txt", "r"):
            if not line.startswith("#"):
                try:
                    svn_rev = int(line.strip())
                    blacklist.add(svn_rev)
                except ValueError, e:
                    pass # not an integer or a comment

    def is_blacklisted(self, svn_rev):
        return svn_rev in self.blacklist

    def reparent(self, module):
        """Reparent the svn transport and return the previous parent."""
        if self.prevmodule == module:
            return module
        svn_url = (self.base + module).encode(self.encoding)
        prevmodule = self.prevmodule
        if prevmodule is None:
            prevmodule = ''
        self.ui.debug("reparent to %s\n" % svn_url)
        svn.ra.reparent(self.ra, svn_url)
        self.prevmodule = module
        return prevmodule

    def expandpaths(self, rev, paths, parents):
        entries = []
        copyfrom = {} # Map of entrypath, revision for finding source of deleted revisions.
        copies = {}

        new_module, revnum = self.revsplit(rev)[1:]
        if new_module != self.module:
            self.module = new_module
            self.reparent(self.module)

        for path, ent in paths:
            entrypath = self.getrelpath(path)
            entry = entrypath.decode(self.encoding)

            kind = self._checkpath(entrypath, revnum)
            if kind == svn.core.svn_node_file:
                entries.append(self.recode(entry))
                if not ent.copyfrom_path or not parents:
                    continue
                # Copy sources not in parent revisions cannot be represented,
                # ignore their origin for now
                pmodule, prevnum = self.revsplit(parents[0])[1:]
                if ent.copyfrom_rev < prevnum:
                    continue
                copyfrom_path = self.getrelpath(ent.copyfrom_path, pmodule)
                if not copyfrom_path:
                    continue
                self.ui.debug("copied to %s from %s@%s\n" %
                              (entrypath, copyfrom_path, ent.copyfrom_rev))
                copies[self.recode(entry)] = self.recode(copyfrom_path)
            elif kind == 0: # gone, but had better be a deleted *file*
                self.ui.debug("gone from %s\n" % ent.copyfrom_rev)

                # if a branch is created but entries are removed in the same
                # changeset, get the right fromrev
                # parents cannot be empty here, you cannot remove things from
                # a root revision.
                uuid, old_module, fromrev = self.revsplit(parents[0])

                basepath = old_module + "/" + self.getrelpath(path)
                entrypath = basepath

                def lookup_parts(p):
                    rc = None
                    parts = p.split("/")
                    for i in range(len(parts)):
                        part = "/".join(parts[:i])
                        info = part, copyfrom.get(part, None)
                        if info[1] is not None:
                            self.ui.debug("Found parent directory %s\n" % info[1])
                            rc = info
                    return rc

                self.ui.debug("base, entry %s %s\n" % (basepath, entrypath))

                frompath, froment = lookup_parts(entrypath) or (None, revnum - 1)

                # need to remove fragment from lookup_parts and replace with copyfrom_path
                if frompath is not None:
                    self.ui.debug("munge-o-matic\n")
                    self.ui.debug(entrypath + '\n')
                    self.ui.debug(entrypath[len(frompath):] + '\n')
                    entrypath = froment.copyfrom_path + entrypath[len(frompath):]
                    fromrev = froment.copyfrom_rev
                    self.ui.debug("Info: %s %s %s %s\n" % (frompath, froment, ent, entrypath))

                # We can avoid the reparent calls if the module has not changed
                # but it probably does not worth the pain.
                prevmodule = self.reparent('')
                fromkind = svn.ra.check_path(self.ra, entrypath.strip('/'), fromrev)
                self.reparent(prevmodule)

                if fromkind == svn.core.svn_node_file:   # a deleted file
                    entries.append(self.recode(entry))
                elif fromkind == svn.core.svn_node_dir:
                    # print "Deleted/moved non-file:", revnum, path, ent
                    # children = self._find_children(path, revnum - 1)
                    # print "find children %s@%d from %d action %s" % (path, revnum, ent.copyfrom_rev, ent.action)
                    # Sometimes this is tricky. For example: in
                    # The Subversion Repository revision 6940 a dir
                    # was copied and one of its files was deleted
                    # from the new location in the same commit. This
                    # code can't deal with that yet.
                    if ent.action == 'C':
                        children = self._find_children(path, fromrev)
                    else:
                        oroot = entrypath.strip('/')
                        nroot = path.strip('/')
                        children = self._find_children(oroot, fromrev)
                        children = [s.replace(oroot,nroot) for s in children]
                    # Mark all [files, not directories] as deleted.
                    for child in children:
                        # Can we move a child directory and its
                        # parent in the same commit? (probably can). Could
                        # cause problems if instead of revnum -1,
                        # we have to look in (copyfrom_path, revnum - 1)
                        entrypath = self.getrelpath("/" + child, module=old_module)
                        if entrypath:
                            entry = self.recode(entrypath.decode(self.encoding))
                            if entry in copies:
                                # deleted file within a copy
                                del copies[entry]
                            else:
                                entries.append(entry)
                else:
                    self.ui.debug('unknown path in revision %d: %s\n' % \
                                  (revnum, path))
            elif kind == svn.core.svn_node_dir:
                # Should probably synthesize normal file entries
                # and handle as above to clean up copy/rename handling.

                # If the directory just had a prop change,
                # then we shouldn't need to look for its children.
                if ent.action == 'M':
                    continue

                # Also this could create duplicate entries. Not sure
                # whether this will matter. Maybe should make entries a set.
                # print "Changed directory", revnum, path, ent.action, ent.copyfrom_path, ent.copyfrom_rev
                # This will fail if a directory was copied
                # from another branch and then some of its files
                # were deleted in the same transaction.
                children = self._find_children(path, revnum)
                children.sort()
                for child in children:
                    # Can we move a child directory and its
                    # parent in the same commit? (probably can). Could
                    # cause problems if instead of revnum -1,
                    # we have to look in (copyfrom_path, revnum - 1)
                    entrypath = self.getrelpath("/" + child)
                    # print child, self.module, entrypath
                    if entrypath:
                        # Need to filter out directories here...
                        kind = self._checkpath(entrypath, revnum)
                        if kind != svn.core.svn_node_dir:
                            entries.append(self.recode(entrypath))

                # Copies here (must copy all from source)
                # Probably not a real problem for us if
                # source does not exist
                if not ent.copyfrom_path or not parents:
                    continue
                # Copy sources not in parent revisions cannot be represented,
                # ignore their origin for now
                pmodule, prevnum = self.revsplit(parents[0])[1:]
                if ent.copyfrom_rev < prevnum:
                    continue
                copyfrompath = ent.copyfrom_path.decode(self.encoding)
                copyfrompath = self.getrelpath(copyfrompath, pmodule)
                if not copyfrompath:
                    continue
                copyfrom[path] = ent
                self.ui.debug("mark %s came from %s:%d\n"
                              % (path, copyfrompath, ent.copyfrom_rev))
                children = self._find_children(ent.copyfrom_path, ent.copyfrom_rev)
                children.sort()
                for child in children:
                    entrypath = self.getrelpath("/" + child, pmodule)
                    if not entrypath:
                        continue
                    entry = entrypath.decode(self.encoding)
                    copytopath = path + entry[len(copyfrompath):]
                    copytopath = self.getrelpath(copytopath)
                    copies[self.recode(copytopath)] = self.recode(entry, pmodule)

        return (util.unique(entries), copies)

    def _fetch_revisions(self, from_revnum, to_revnum):
        if from_revnum < to_revnum:
            from_revnum, to_revnum = to_revnum, from_revnum

        self.child_cset = None

        def isdescendantof(parent, child):
            if not child or not parent or not child.startswith(parent):
                return False
            subpath = child[len(parent):]
            return len(subpath) > 1 and subpath[0] == '/'

        def parselogentry(orig_paths, revnum, author, date, message):
            """Return the parsed commit object or None, and True if
            the revision is a branch root.
            """
            self.ui.debug("parsing revision %d (%d changes)\n" %
                          (revnum, len(orig_paths)))

            branched = False
            rev = self.revid(revnum)
            # branch log might return entries for a parent we already have

            if (rev in self.commits or revnum < to_revnum):
                return None, branched

            parents = []
            # check whether this revision is the start of a branch or part
            # of a branch renaming
            orig_paths = orig_paths.items()
            orig_paths.sort()
            root_paths = [(p,e) for p,e in orig_paths if self.module.startswith(p)]
            if root_paths:
                path, ent = root_paths[-1]
                if ent.copyfrom_path:
                    # If dir was moved while one of its file was removed
                    # the log may look like:
                    # A /dir   (from /dir:x)
                    # A /dir/a (from /dir/a:y)
                    # A /dir/b (from /dir/b:z)
                    # ...
                    # for all remaining children.
                    # Let's take the highest child element from rev as source.
                    copies = [(p,e) for p,e in orig_paths[:-1]
                          if isdescendantof(ent.copyfrom_path, e.copyfrom_path)]
                    fromrev = max([e.copyfrom_rev for p,e in copies] + [ent.copyfrom_rev])
                    branched = True
                    newpath = ent.copyfrom_path + self.module[len(path):]
                    # ent.copyfrom_rev may not be the actual last revision
                    previd = self.latest(newpath, fromrev)
                    if previd is not None:
                        prevmodule, prevnum = self.revsplit(previd)[1:]
                        if prevnum >= self.startrev:
                            parents = [previd]
                            self.ui.note('found parent of branch %s at %d: %s\n' %
                                         (self.module, prevnum, prevmodule))
                else:
                    self.ui.debug("No copyfrom path, don't know what to do.\n")

            paths = []
            # filter out unrelated paths
            for path, ent in orig_paths:
                if self.getrelpath(path) is None:
                    continue
                paths.append((path, ent))

            # Example SVN datetime. Includes microseconds.
            # ISO-8601 conformant
            # '2007-01-04T17:35:00.902377Z'
            date = util.parsedate(date[:19] + " UTC", ["%Y-%m-%dT%H:%M:%S"])

            log = message and self.recode(message) or ''
            author = author and self.recode(author) or ''
            try:
                branch = self.module.split("/")[-1]
                if branch == 'trunk':
                    branch = ''
            except IndexError:
                branch = None

            cset = commit(author=author,
                          date=util.datestr(date),
                          desc=log,
                          parents=parents,
                          branch=branch,
                          rev=rev.encode('utf-8'))

            self.commits[rev] = cset
            # The parents list is *shared* among self.paths and the
            # commit object. Both will be updated below.
            self.paths[rev] = (paths, cset.parents)
            if self.child_cset and not self.child_cset.parents:
                self.child_cset.parents[:] = [rev]
            self.child_cset = cset
            return cset, branched

        self.ui.note('fetching revision log for "%s" from %d to %d\n' %
                     (self.module, from_revnum, to_revnum))

        try:
            firstcset = None
            lastonbranch = False
            stream = self._getlog([self.module], from_revnum, to_revnum)
            try:
                for entry in stream:
                    paths, revnum, author, date, message = entry
                    if revnum < self.startrev:
                        lastonbranch = True
                        break
                    if self.is_blacklisted(revnum):
                        self.ui.note('skipping blacklisted revision %d\n'
                                     % revnum)
                        continue
                    if paths is None:
                        self.ui.debug('revision %d has no entries\n' % revnum)
                        continue
                    cset, lastonbranch = parselogentry(paths, revnum, author,
                                                       date, message)
                    if cset:
                        firstcset = cset
                    if lastonbranch:
                        break
            finally:
                stream.close()

            if not lastonbranch and firstcset and not firstcset.parents:
                # The first revision of the sequence (the last fetched one)
                # has invalid parents if not a branch root. Find the parent
                # revision now, if any.
                try:
                    firstrevnum = self.revnum(firstcset.rev)
                    if firstrevnum > 1:
                        latest = self.latest(self.module, firstrevnum - 1)
                        if latest:
                            firstcset.parents.append(latest)
                except util.Abort:
                    pass
        except SubversionException, (inst, num):
            if num == svn.core.SVN_ERR_FS_NO_SUCH_REVISION:
                raise util.Abort('svn: branch has no revision %s' % to_revnum)
            raise

    def _getfile(self, file, rev):
        io = StringIO()
        # TODO: ra.get_file transmits the whole file instead of diffs.
        mode = ''
        try:
            new_module, revnum = self.revsplit(rev)[1:]
            if self.module != new_module:
                self.module = new_module
                self.reparent(self.module)
            info = svn.ra.get_file(self.ra, file, revnum, io)
            if isinstance(info, list):
                info = info[-1]
            mode = ("svn:executable" in info) and 'x' or ''
            mode = ("svn:special" in info) and 'l' or mode
        except SubversionException, e:
            notfound = (svn.core.SVN_ERR_FS_NOT_FOUND,
                svn.core.SVN_ERR_RA_DAV_PATH_NOT_FOUND)
            if e.apr_err in notfound: # File not found
                raise IOError()
            raise
        data = io.getvalue()
        if mode == 'l':
            link_prefix = "link "
            if data.startswith(link_prefix):
                data = data[len(link_prefix):]
        return data, mode

    def _find_children(self, path, revnum):
        path = path.strip('/')
        pool = Pool()
        rpath = '/'.join([self.base, path]).strip('/')
        return ['%s/%s' % (path, x) for x in svn.client.ls(rpath, optrev(revnum), True, self.ctx, pool).keys()]

    def getrelpath(self, path, module=None):
        if module is None:
            module = self.module
        # Given the repository url of this wc, say
        #   "http://server/plone/CMFPlone/branches/Plone-2_0-branch"
        # extract the "entry" portion (a relative path) from what
        # svn log --xml says, ie
        #   "/CMFPlone/branches/Plone-2_0-branch/tests/PloneTestCase.py"
        # that is to say "tests/PloneTestCase.py"
        if path.startswith(module):
            relative = path.rstrip('/')[len(module):]
            if relative.startswith('/'):
                return relative[1:]
            elif relative == '':
                return relative

        # The path is outside our tracked tree...
        self.ui.debug('%r is not under %r, ignoring\n' % (path, module))
        return None

    def _checkpath(self, path, revnum):
        # ra.check_path does not like leading slashes very much, it leads
        # to PROPFIND subversion errors
        return svn.ra.check_path(self.ra, path.strip('/'), revnum)

    def _getlog(self, paths, start, end, limit=0, discover_changed_paths=True,
                strict_node_history=False):
        # Normalize path names, svn >= 1.5 only wants paths relative to
        # supplied URL
        relpaths = []
        for p in paths:
            if not p.startswith('/'):
                p = self.module + '/' + p
            relpaths.append(p.strip('/'))
        args = [self.base, relpaths, start, end, limit, discover_changed_paths,
                strict_node_history]
        arg = encodeargs(args)
        hgexe = util.hgexecutable()
        cmd = '%s debugsvnlog' % util.shellquote(hgexe)
        stdin, stdout = os.popen2(cmd, 'b')
        stdin.write(arg)
        stdin.close()
        return logstream(stdout)

pre_revprop_change = '''#!/bin/sh

REPOS="$1"
REV="$2"
USER="$3"
PROPNAME="$4"
ACTION="$5"

if [ "$ACTION" = "M" -a "$PROPNAME" = "svn:log" ]; then exit 0; fi
if [ "$ACTION" = "A" -a "$PROPNAME" = "hg:convert-branch" ]; then exit 0; fi
if [ "$ACTION" = "A" -a "$PROPNAME" = "hg:convert-rev" ]; then exit 0; fi

echo "Changing prohibited revision property" >&2
exit 1
'''

class svn_sink(converter_sink, commandline):
    commit_re = re.compile(r'Committed revision (\d+).', re.M)

    def prerun(self):
        if self.wc:
            os.chdir(self.wc)

    def postrun(self):
        if self.wc:
            os.chdir(self.cwd)

    def join(self, name):
        return os.path.join(self.wc, '.svn', name)

    def revmapfile(self):
        return self.join('hg-shamap')

    def authorfile(self):
        return self.join('hg-authormap')

    def __init__(self, ui, path):
        converter_sink.__init__(self, ui, path)
        commandline.__init__(self, ui, 'svn')
        self.delete = []
        self.setexec = []
        self.delexec = []
        self.copies = []
        self.wc = None
        self.cwd = os.getcwd()

        path = os.path.realpath(path)

        created = False
        if os.path.isfile(os.path.join(path, '.svn', 'entries')):
            self.wc = path
            self.run0('update')
        else:
            wcpath = os.path.join(os.getcwd(), os.path.basename(path) + '-wc')

            if os.path.isdir(os.path.dirname(path)):
                if not os.path.exists(os.path.join(path, 'db', 'fs-type')):
                    ui.status(_('initializing svn repo %r\n') %
                              os.path.basename(path))
                    commandline(ui, 'svnadmin').run0('create', path)
                    created = path
                path = util.normpath(path)
                if not path.startswith('/'):
                    path = '/' + path
                path = 'file://' + path

            ui.status(_('initializing svn wc %r\n') % os.path.basename(wcpath))
            self.run0('checkout', path, wcpath)

            self.wc = wcpath
        self.opener = util.opener(self.wc)
        self.wopener = util.opener(self.wc)
        self.childmap = mapfile(ui, self.join('hg-childmap'))
        self.is_exec = util.checkexec(self.wc) and util.is_exec or None

        if created:
            hook = os.path.join(created, 'hooks', 'pre-revprop-change')
            fp = open(hook, 'w')
            fp.write(pre_revprop_change)
            fp.close()
            util.set_flags(hook, "x")

        xport = transport.SvnRaTransport(url=geturl(path))
        self.uuid = svn.ra.get_uuid(xport.ra)

    def wjoin(self, *names):
        return os.path.join(self.wc, *names)

    def putfile(self, filename, flags, data):
        if 'l' in flags:
            self.wopener.symlink(data, filename)
        else:
            try:
                if os.path.islink(self.wjoin(filename)):
                    os.unlink(filename)
            except OSError:
                pass
            self.wopener(filename, 'w').write(data)

            if self.is_exec:
                was_exec = self.is_exec(self.wjoin(filename))
            else:
                # On filesystems not supporting execute-bit, there is no way
                # to know if it is set but asking subversion. Setting it
                # systematically is just as expensive and much simpler.
                was_exec = 'x' not in flags

            util.set_flags(self.wjoin(filename), flags)
            if was_exec:
                if 'x' not in flags:
                    self.delexec.append(filename)
            else:
                if 'x' in flags:
                    self.setexec.append(filename)

    def delfile(self, name):
        self.delete.append(name)

    def copyfile(self, source, dest):
        self.copies.append([source, dest])

    def _copyfile(self, source, dest):
        # SVN's copy command pukes if the destination file exists, but
        # our copyfile method expects to record a copy that has
        # already occurred.  Cross the semantic gap.
        wdest = self.wjoin(dest)
        exists = os.path.exists(wdest)
        if exists:
            fd, tempname = tempfile.mkstemp(
                prefix='hg-copy-', dir=os.path.dirname(wdest))
            os.close(fd)
            os.unlink(tempname)
            os.rename(wdest, tempname)
        try:
            self.run0('copy', source, dest)
        finally:
            if exists:
                try:
                    os.unlink(wdest)
                except OSError:
                    pass
                os.rename(tempname, wdest)

    def dirs_of(self, files):
        dirs = util.set()
        for f in files:
            if os.path.isdir(self.wjoin(f)):
                dirs.add(f)
            for i in strutil.rfindall(f, '/'):
                dirs.add(f[:i])
        return dirs

    def add_dirs(self, files):
        add_dirs = [d for d in self.dirs_of(files)
                    if not os.path.exists(self.wjoin(d, '.svn', 'entries'))]
        if add_dirs:
            add_dirs.sort()
            self.xargs(add_dirs, 'add', non_recursive=True, quiet=True)
        return add_dirs

    def add_files(self, files):
        if files:
            self.xargs(files, 'add', quiet=True)
        return files

    def tidy_dirs(self, names):
        dirs = list(self.dirs_of(names))
        dirs.sort()
        dirs.reverse()
        deleted = []
        for d in dirs:
            wd = self.wjoin(d)
            if os.listdir(wd) == '.svn':
                self.run0('delete', d)
                deleted.append(d)
        return deleted

    def addchild(self, parent, child):
        self.childmap[parent] = child

    def revid(self, rev):
        return u"svn:%s@%s" % (self.uuid, rev)

    def putcommit(self, files, parents, commit):
        for parent in parents:
            try:
                return self.revid(self.childmap[parent])
            except KeyError:
                pass
        entries = util.set(self.delete)
        files = util.frozenset(files)
        entries.update(self.add_dirs(files.difference(entries)))
        if self.copies:
            for s, d in self.copies:
                self._copyfile(s, d)
            self.copies = []
        if self.delete:
            self.xargs(self.delete, 'delete')
            self.delete = []
        entries.update(self.add_files(files.difference(entries)))
        entries.update(self.tidy_dirs(entries))
        if self.delexec:
            self.xargs(self.delexec, 'propdel', 'svn:executable')
            self.delexec = []
        if self.setexec:
            self.xargs(self.setexec, 'propset', 'svn:executable', '*')
            self.setexec = []

        fd, messagefile = tempfile.mkstemp(prefix='hg-convert-')
        fp = os.fdopen(fd, 'w')
        fp.write(commit.desc)
        fp.close()
        try:
            output = self.run0('commit',
                               username=util.shortuser(commit.author),
                               file=messagefile,
                               encoding='utf-8')
            try:
                rev = self.commit_re.search(output).group(1)
            except AttributeError:
                self.ui.warn(_('unexpected svn output:\n'))
                self.ui.warn(output)
                raise util.Abort(_('unable to cope with svn output'))
            if commit.rev:
                self.run('propset', 'hg:convert-rev', commit.rev,
                         revprop=True, revision=rev)
            if commit.branch and commit.branch != 'default':
                self.run('propset', 'hg:convert-branch', commit.branch,
                         revprop=True, revision=rev)
            for parent in parents:
                self.addchild(parent, rev)
            return self.revid(rev)
        finally:
            os.unlink(messagefile)

    def puttags(self, tags):
        self.ui.warn(_('XXX TAGS NOT IMPLEMENTED YET\n'))
