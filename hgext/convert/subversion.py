# Subversion 1.4/1.5 Python API backend
#
# Copyright(C) 2007 Daniel Holth et al

import pprint
import locale

from mercurial import util

# Subversion stuff. Works best with very recent Python SVN bindings
# e.g. SVN 1.5 or backports. Thanks to the bzr folks for enhancing
# these bindings.

from cStringIO import StringIO

from common import NoRepo, commit, converter_source

try:
    from svn.core import SubversionException, Pool
    import svn.core
    import svn.ra
    import svn.delta
    import svn
    import transport
except ImportError:
    pass

class CompatibilityException(Exception): pass

LOG_BATCH_SIZE = 50

class svn_entry(object):
    """Emulate a Subversion path change."""
    __slots__ = ['path', 'copyfrom_path', 'copyfrom_rev', 'action']
    def __init__(self, entry):
        self.copyfrom_path = entry.copyfrom_path
        self.copyfrom_rev = entry.copyfrom_rev
        self.action = entry.action

    def __str__(self):
        return "%s %s %s" % (self.action, self.copyfrom_path, self.copyfrom_rev)

    def __repr__(self):
        return self.__str__()

class svn_paths(object):
    """Emulate a Subversion ordered dictionary of changed paths."""
    __slots__ = ['values', 'order']
    def __init__(self, orig_paths):
        self.order = []
        self.values = {}
        if hasattr(orig_paths, 'keys'):
            self.order = sorted(orig_paths.keys())
            self.values.update(orig_paths)
            return
        if not orig_paths:
            return
        for path in orig_paths:
            self.order.append(path)
            self.values[path] = svn_entry(orig_paths[path])
        self.order.sort() # maybe the order it came in isn't so great...

    def __iter__(self):
        return iter(self.order)

    def __getitem__(self, key):
        return self.values[key]

    def __str__(self):
        s = "{\n"
        for path in self.order:
            s += "'%s': %s,\n" % (path, self.values[path])
        s += "}"
        return s
    
    def __repr__(self):
        return self.__str__()

# SVN conversion code stolen from bzr-svn and tailor
class convert_svn(converter_source):
    def __init__(self, ui, url, rev=None):
        try:
            SubversionException
        except NameError:
            msg = 'subversion python bindings could not be loaded\n'
            ui.warn(msg)
            raise NoRepo(msg)

        self.ui = ui
        self.encoding = locale.getpreferredencoding()
        latest = None
        if rev:
            try:
                latest = int(rev)
            except ValueError:
                raise util.Abort('svn: revision %s is not an integer' % rev)
        try:
            # Support file://path@rev syntax. Useful e.g. to convert
            # deleted branches.
            url, latest = url.rsplit("@", 1)
            latest = int(latest)
        except ValueError, e:
            pass
        self.url = url
        self.encoding = 'UTF-8' # Subversion is always nominal UTF-8
        try:
            self.transport = transport.SvnRaTransport(url = url)
            self.ra = self.transport.ra
            self.base = svn.ra.get_repos_root(self.ra)
            self.module = self.url[len(self.base):]
            self.modulemap = {} # revision, module
            self.commits = {}
            self.files = {}
            self.uuid = svn.ra.get_uuid(self.ra).decode(self.encoding)
        except SubversionException, e:
            raise NoRepo("couldn't open SVN repo %s" % url)

        try:
            self.get_blacklist()
        except IOError, e:
            pass

        self.last_changed = self.latest(self.module, latest)

        self.head = self.rev(self.last_changed)

    def rev(self, revnum):
        return (u"svn:%s%s@%s" % (self.uuid, self.module, revnum)).decode(self.encoding)

    def revnum(self, rev):
        return int(rev.split('@')[-1])

    def latest(self, path, stop=0):
        'find the latest revision affecting path, up to stop'
        if not stop:
            stop = svn.ra.get_latest_revnum(self.ra)
        try:
            self.reparent('')
            dirent = svn.ra.stat(self.ra, path.strip('/'), stop)
            self.reparent(self.module)
        except SubversionException:
            dirent = None
        if not dirent:
            raise util.Abort('module %s not found up to revision %d' \
                             % (self.module, stop))

        return dirent.created_rev

    def get_blacklist(self):
        """Avoid certain revision numbers.
        It is not uncommon for two nearby revisions to cancel each other
        out, e.g. 'I copied trunk into a subdirectory of itself instead
        of making a branch'. The converted repository is significantly
        smaller if we ignore such revisions."""
        self.blacklist = set()
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
        svn_url = self.base + module
        self.ui.debug("reparent to %s\n" % svn_url.encode(self.encoding))
        svn.ra.reparent(self.ra, svn_url.encode(self.encoding))

    def _fetch_revisions(self, from_revnum = 0, to_revnum = 347, pb=None):
        # batching is broken for branches
        to_revnum = 0
        if not hasattr(self, 'child_rev'):
            self.child_rev = from_revnum
            self.child_cset = self.commits.get(self.child_rev)
        else:
            self.commits[self.child_rev] = self.child_cset
            # batching broken
            return
            # if the branch was created in the middle of the last batch,
            # svn log will complain that the path doesn't exist in this batch
            # so we roll the parser back to the last revision where this branch appeared
            revnum = self.revnum(self.child_rev)
            if revnum > from_revnum:
                from_revnum = revnum

        self.ui.debug('Fetching revisions %d to %d\n' % (from_revnum, to_revnum))

        def get_entry_from_path(path, module=self.module):
            # Given the repository url of this wc, say
            #   "http://server/plone/CMFPlone/branches/Plone-2_0-branch"
            # extract the "entry" portion (a relative path) from what
            # svn log --xml says, ie
            #   "/CMFPlone/branches/Plone-2_0-branch/tests/PloneTestCase.py"
            # that is to say "tests/PloneTestCase.py"

            if path.startswith(module):
                relative = path[len(module):]
                if relative.startswith('/'):
                    return relative[1:]
                else:
                    return relative

            # The path is outside our tracked tree...
            self.ui.debug('Ignoring %r since it is not under %r\n' % (path, module))
            return None

        received = []
        def rcvr(*arg, **args):
            orig_paths, revnum, author, date, message, pool = arg
            new_orig_paths = svn_paths(orig_paths)
            rcvr2(new_orig_paths, revnum, author, date, message, pool)

        def rcvr2(orig_paths, revnum, author, date, message, pool, better_paths = None):
            if not self.is_blacklisted(revnum):
                received.append((orig_paths, revnum, author, date, message))
           
        def after_received(orig_paths, revnum, author, date, message):
            if revnum in self.modulemap:
                new_module = self.modulemap[revnum]
                if new_module != self.module:
                    self.module = new_module
                    self.reparent(self.module)

            copyfrom = {} # Map of entrypath, revision for finding source of deleted revisions.
            copies = {}
            entries = []
            self.ui.debug("Parsing revision %d\n" % revnum)
            if orig_paths is None:
                return

            rev = self.rev(revnum)
            try:
                branch = self.module.split("/")[-1]
                if branch == 'trunk':
                    branch = ''
            except IndexError:
                branch = None
                
            for path in orig_paths:
                # self.ui.write("path %s\n" % path)
                if path == self.module: # Follow branching back in history
                    ent = orig_paths[path]
                    if ent:
                        if ent.copyfrom_path:
                            self.modulemap[ent.copyfrom_rev] = ent.copyfrom_path
                        else:
                            self.ui.debug("No copyfrom path, don't know what to do.\n")
                            # Maybe it was added and there is no more history.
                entrypath = get_entry_from_path(path, module=self.module)
                # self.ui.write("entrypath %s\n" % entrypath)
                if entrypath is None:
                    # Outside our area of interest
                    self.ui.debug("boring@%s: %s\n" % (revnum, path))
                    continue
                entry = entrypath.decode(self.encoding)
                ent = orig_paths[path]
                if not entrypath:
                    # TODO: branch creation event
                    pass

                kind = svn.ra.check_path(self.ra, entrypath, revnum)
                if kind == svn.core.svn_node_file:
                    if ent.copyfrom_path:
                        copyfrom_path = get_entry_from_path(ent.copyfrom_path)
                        if copyfrom_path:
                            self.ui.debug("Copied to %s from %s@%s\n" % (entry, copyfrom_path, ent.copyfrom_rev))
                            # It's probably important for hg that the source
                            # exists in the revision's parent, not just the
                            # ent.copyfrom_rev
                            fromkind = svn.ra.check_path(self.ra, copyfrom_path, ent.copyfrom_rev)
                            if fromkind != 0:
                                copies[self.recode(entry)] = self.recode(copyfrom_path)
                    entries.append(self.recode(entry))
                elif kind == 0: # gone, but had better be a deleted *file*
                    self.ui.debug("gone from %s\n" % ent.copyfrom_rev)

                    fromrev = revnum - 1
                    # might always need to be revnum - 1 in these 3 lines?
                    old_module = self.modulemap.get(fromrev, self.module)
                    basepath = old_module + "/" + get_entry_from_path(path, module=self.module)
                    entrypath = old_module + "/" + get_entry_from_path(path, module=self.module)

                    def lookup_parts(p):
                        rc = None
                        parts = p.split("/")
                        for i in range(len(parts)):
                            part = "/".join(parts[:i])
                            info = part, copyfrom.get(part, None)
                            if info[1] is not None:
                                self.ui.debug("Found parent directory %s\n" % info)
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

                    fromkind = svn.ra.check_path(self.ra, entrypath, fromrev)
                    if fromkind == svn.core.svn_node_file:   # a deleted file
                        entries.append(self.recode(entry))
                    else:
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
                            entrypath = get_entry_from_path("/" + child, module=old_module)
                            if entrypath:
                                entry = self.recode(entrypath.decode(self.encoding))
                                if entry in copies:
                                    # deleted file within a copy
                                    del copies[entry]
                                else:
                                    entries.append(entry)
                elif kind == svn.core.svn_node_dir:
                    # Should probably synthesize normal file entries
                    # and handle as above to clean up copy/rename handling.

                    # If the directory just had a prop change,
                    # then we shouldn't need to look for its children.
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
                        entrypath = get_entry_from_path("/" + child, module=self.module)
                        # print child, self.module, entrypath
                        if entrypath:
                            # Need to filter out directories here...
                            kind = svn.ra.check_path(self.ra, entrypath, revnum)
                            if kind != svn.core.svn_node_dir:
                                entries.append(self.recode(entrypath))

                    # Copies here (must copy all from source)
                    # Probably not a real problem for us if
                    # source does not exist

                    # Can do this with the copy command "hg copy"
                    # if ent.copyfrom_path:
                    #     copyfrom_entry = get_entry_from_path(ent.copyfrom_path.decode(self.encoding),
                    #             module=self.module)
                    #     copyto_entry = entrypath
                    #
                    #     print "copy directory", copyfrom_entry, 'to', copyto_entry
                    #
                    #     copies.append((copyfrom_entry, copyto_entry))

                    if ent.copyfrom_path:
                        copyfrom_path = ent.copyfrom_path.decode(self.encoding)
                        copyfrom_entry = get_entry_from_path(copyfrom_path, module=self.module)
                        if copyfrom_entry:
                            copyfrom[path] = ent
                            self.ui.debug("mark %s came from %s\n" % (path, copyfrom[path]))

                            # Good, /probably/ a regular copy. Really should check
                            # to see whether the parent revision actually contains
                            # the directory in question.
                            children = self._find_children(self.recode(copyfrom_path), ent.copyfrom_rev)
                            children.sort()
                            for child in children:
                                entrypath = get_entry_from_path("/" + child, module=self.module)
                                if entrypath:
                                    entry = entrypath.decode(self.encoding)
                                    # print "COPY COPY From", copyfrom_entry, entry
                                    copyto_path = path + entry[len(copyfrom_entry):]
                                    copyto_entry =  get_entry_from_path(copyto_path, module=self.module)
                                    # print "COPY", entry, "COPY To", copyto_entry
                                    copies[self.recode(copyto_entry)] = self.recode(entry)
                                    # copy from quux splort/quuxfile

            self.modulemap[revnum] = self.module # track backwards in time
            # a list of (filename, id) where id lets us retrieve the file.
            # eg in git, id is the object hash. for svn it'll be the 
            self.files[rev] = zip(entries, [rev] * len(entries))
            if not entries:
                return

            # Example SVN datetime. Includes microseconds.
            # ISO-8601 conformant
            # '2007-01-04T17:35:00.902377Z'
            date = util.parsedate(date[:18] + " UTC", ["%Y-%m-%dT%H:%M:%S"])

            log = message and self.recode(message)
            author = author and self.recode(author) or ''

            cset = commit(author=author,
                          date=util.datestr(date), 
                          desc=log, 
                          parents=[],
                          copies=copies,
                          branch=branch)

            if self.child_cset and self.child_rev != rev:
                self.child_cset.parents = [rev]
                self.commits[self.child_rev] = self.child_cset
            self.child_cset = cset
            self.child_rev = rev

        try:
            discover_changed_paths = True
            strict_node_history = False
            svn.ra.get_log(self.ra, [self.module], from_revnum, to_revnum, 
                           0, discover_changed_paths, strict_node_history, rcvr)
            for args in received:
                after_received(*args)
            self.last_revnum = to_revnum
        except SubversionException, (_, num):
            if num == svn.core.SVN_ERR_FS_NO_SUCH_REVISION:
                raise NoSuchRevision(branch=self, 
                    revision="Revision number %d" % to_revnum)
            raise

    def getheads(self):
        # svn-url@rev
        # Not safe if someone committed:
        self.heads = [self.head]
        # print self.commits.keys()
        return self.heads

    def _getfile(self, file, rev):
        io = StringIO()
        # TODO: ra.get_file transmits the whole file instead of diffs.
        mode = ''
        try:
            revnum = self.revnum(rev)
            if self.module != self.modulemap[revnum]:
                self.module = self.modulemap[revnum]
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

    def getfile(self, file, rev):
        data, mode = self._getfile(file, rev)
        self.modecache[(file, rev)] = mode
        return data

    def getmode(self, file, rev):        
        return self.modecache[(file, rev)]

    def getchanges(self, rev):
        self.modecache = {}
        files = self.files[rev]
        cl = files
        cl.sort()
        return cl

    def getcommit(self, rev):
        if rev not in self.commits:
            revnum = self.revnum(rev)
            minrev = revnum - LOG_BATCH_SIZE > 0 and revnum - LOG_BATCH_SIZE or 0
            self._fetch_revisions(from_revnum=revnum, to_revnum=minrev)
        return self.commits[rev]

    def gettags(self):
        return []

    def _find_children(self, path, revnum):
        path = path.strip("/")

        def _find_children_fallback(path, revnum):
            # SWIG python bindings for getdir are broken up to at least 1.4.3
            if not hasattr(self, 'client_ctx'):
                self.client_ctx = svn.client.create_context()
            pool = Pool()
            optrev = svn.core.svn_opt_revision_t()
            optrev.kind = svn.core.svn_opt_revision_number
            optrev.value.number = revnum
            rpath = '/'.join([self.base, path]).strip('/')
            return ['%s/%s' % (path, x) for x in svn.client.ls(rpath, optrev, True, self.client_ctx, pool).keys()]

        if hasattr(self, '_find_children_fallback'):
            return _find_children_fallback(path, revnum)

        self.reparent("/" + path)
        pool = Pool()

        children = []
        def find_children_inner(children, path, revnum = revnum):
            if hasattr(svn.ra, 'get_dir2'): # Since SVN 1.4
                fields = 0xffffffff # Binding does not provide SVN_DIRENT_ALL
                getdir = svn.ra.get_dir2(self.ra, path, revnum, fields, pool)
            else:
                getdir = svn.ra.get_dir(self.ra, path, revnum, pool)
            if type(getdir) == dict:
                # python binding for getdir is broken up to at least 1.4.3
                raise CompatibilityException()
            dirents = getdir[0]
            if type(dirents) == int:
                # got here once due to infinite recursion bug
                # pprint.pprint(getdir)
                return
            c = dirents.keys()
            c.sort()
            for child in c:
                dirent = dirents[child]
                if dirent.kind == svn.core.svn_node_dir:
                    find_children_inner(children, (path + "/" + child).strip("/"))
                else:
                    children.append((path + "/" + child).strip("/"))

        try:
            find_children_inner(children, "")
        except CompatibilityException:
            self._find_children_fallback = True
            self.reparent(self.module)
            return _find_children_fallback(path, revnum)

        self.reparent(self.module)
        return [path + "/" + c for c in children]
