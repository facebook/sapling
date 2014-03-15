import posixpath
import os
import tempfile

from mercurial import context
from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node

import util
import maps
import layouts
import editor


class SVNMeta(object):

    def __init__(self, repo, uuid=None, subdir=None):
        """path is the path to the target hg repo.

        subdir is the subdirectory of the edits *on the svn server*.
        It is needed for stripping paths off in certain cases.
        """
        self.ui = repo.ui
        self.repo = repo
        self.path = os.path.normpath(repo.join('..'))

        if not os.path.isdir(self.meta_data_dir):
            os.makedirs(self.meta_data_dir)
        self.uuid = uuid
        self.subdir = subdir
        self.revmap = maps.RevMap(repo)

        author_host = self.ui.config('hgsubversion', 'defaulthost', uuid)
        authors = util.configpath(self.ui, 'authormap')
        self.usebranchnames = self.ui.configbool('hgsubversion',
                                                 'usebranchnames', True)
        branchmap = util.configpath(self.ui, 'branchmap')
        tagmap = util.configpath(self.ui, 'tagmap')
        filemap = util.configpath(self.ui, 'filemap')

        self.branches = util.load(self.branch_info_file) or {}
        self.prevbranches = dict(self.branches)
        self.tags = maps.Tags(repo)
        self._layout = layouts.detect.layout_from_file(self.meta_data_dir,
                                                       ui=self.repo.ui)
        self._layoutobj = None

        self.authors = maps.AuthorMap(self.ui, self.authors_file,
                                 defaulthost=author_host)
        if authors: self.authors.load(authors)

        self.branchmap = maps.BranchMap(self.ui, self.branchmapfile)
        if branchmap:
            self.branchmap.load(branchmap)

        self.tagmap = maps.TagMap(self.ui, self.tagmapfile)
        if tagmap:
            self.tagmap.load(tagmap)

        self.filemap = maps.FileMap(self.ui, self.filemap_file)
        if filemap:
            self.filemap.load(filemap)

        self.lastdate = '1970-01-01 00:00:00 -0000'
        self.addedtags = {}
        self.deletedtags = {}

    @property
    def layout(self):
        # this method can't determine the layout, but it needs to be
        # resolved into something other than auto before this ever
        # gets called
        if not self._layout or self._layout == 'auto':
            self._layout = layouts.detect.layout_from_config(self.repo.ui)
            layouts.persist.layout_to_file(self.meta_data_dir, self._layout)
        return self._layout

    @property
    def layoutobj(self):
        if not self._layoutobj:
            self._layoutobj = layouts.layout_from_name(self.layout, self.ui)
        return self._layoutobj

    @property
    def editor(self):
        if not hasattr(self, '_editor'):
            self._editor = editor.HgEditor(self)
        return self._editor

    def _get_subdir(self):
        return self.__subdir

    def _set_subdir(self, subdir):
        if subdir:
            subdir = '/'.join(p for p in subdir.split('/') if p)

        subdirfile = os.path.join(self.meta_data_dir, 'subdir')

        if os.path.isfile(subdirfile):
            stored_subdir = open(subdirfile).read()
            assert stored_subdir is not None
            if subdir is None:
                self.__subdir = stored_subdir
            elif subdir != stored_subdir:
                raise hgutil.Abort('unable to work on a different path in the '
                                   'repository')
            else:
                self.__subdir = subdir
        elif subdir is not None:
            f = open(subdirfile, 'w')
            f.write(subdir)
            f.close()
            self.__subdir = subdir
        else:
            raise hgutil.Abort("hgsubversion metadata unavailable; "
                               "please run 'hg svn rebuildmeta'")

    subdir = property(_get_subdir, _set_subdir, None,
                    'Error-checked sub-directory of source Subversion '
                    'repository.')

    def _get_uuid(self):
        return self.__uuid

    def _set_uuid(self, uuid):
        uuidfile = os.path.join(self.meta_data_dir, 'uuid')
        if os.path.isfile(uuidfile):
            stored_uuid = util.load(uuidfile)
            assert stored_uuid
            if uuid and uuid != stored_uuid:
                raise hgutil.Abort('unable to operate on unrelated repository')
            self.__uuid = uuid or stored_uuid
        elif uuid:
            util.dump(uuid, uuidfile)
            self.__uuid = uuid
        else:
            raise hgutil.Abort("hgsubversion metadata unavailable; "
                               "please run 'hg svn rebuildmeta'")

    uuid = property(_get_uuid, _set_uuid, None,
                    'Error-checked UUID of source Subversion repository.')

    @property
    def meta_data_dir(self):
        return os.path.join(self.path, '.hg', 'svn')

    @property
    def branch_info_file(self):
        return os.path.join(self.meta_data_dir, 'branch_info')

    @property
    def authors_file(self):
        return os.path.join(self.meta_data_dir, 'authors')

    @property
    def filemap_file(self):
        return os.path.join(self.meta_data_dir, 'filemap')

    @property
    def branchmapfile(self):
        return os.path.join(self.meta_data_dir, 'branchmap')

    @property
    def tagmapfile(self):
        # called tag-renames for backwards compatibility
        return os.path.join(self.meta_data_dir, 'tag-renames')

    def fixdate(self, date):
        if date is not None:
            date = date.replace('T', ' ').replace('Z', '').split('.')[0]
            date += ' -0000'
            self.lastdate = date
        else:
            date = self.lastdate
        return date

    def save(self):
        '''Save the Subversion metadata. This should really be called after
        every revision is created.
        '''
        util.dump(self.branches, self.branch_info_file)

    def localname(self, path):
        """Compute the local name for a branch located at path.
        """
        return self.layoutobj.localname(path)

    def remotename(self, branch):
        return self.layoutobj.remotename(branch)

    def genextra(self, revnum, branch):
        extra = {}
        subdir = self.subdir
        if subdir and subdir[-1] == '/':
            subdir = subdir[:-1]
        if subdir and subdir[0] != '/':
            subdir = '/' + subdir

        path = self.layoutobj.remotepath(branch, subdir)

        if branch:
            extra['branch'] = branch

        extra['convert_revision'] = 'svn:%(uuid)s%(path)s@%(rev)s' % {
            'uuid': self.uuid,
            'path': path,
            'rev': revnum,
        }
        return extra

    def mapbranch(self, extra, close=False):
        if close:
            extra['close'] = 1
        mapped = self.branchmap.get(extra.get('branch', 'default'))
        if not self.usebranchnames or mapped == 'default':
            extra.pop('branch', None)
        elif mapped:
            extra['branch'] = mapped

        if extra.get('branch') == 'default':
            extra.pop('branch', None)

    def normalize(self, path):
        '''Normalize a path to strip of leading slashes and our subdir if we
        have one.
        '''
        if self.subdir and path == self.subdir[:-1]:
            return ''
        if path and path[0] == '/':
            path = path[1:]
        if path == self.subdir:
            return ''
        if path and path.startswith(self.subdir + '/'):
            path = path[len(self.subdir):]
        if path and path[0] == '/':
            path = path[1:]
        return path

    @property
    def taglocations(self):
        return self.layoutobj.taglocations(self.meta_data_dir)

    def get_path_tag(self, path):
        """If path could represent the path to a tag, returns the
        potential (non-empty) tag name. Otherwise, returns None

        Note that it's only a tag if it was copied from the path '' in a branch
        (or tag) we have, for our purposes.
        """
        path = self.normalize(path)
        return self.layoutobj.get_path_tag(path, self.taglocations)

    def split_branch_path(self, path, existing=True):
        """Figure out which branch inside our repo this path represents, and
        also figure out which path inside that branch it is.

        Returns a tuple of (path within branch, local branch name, server-side
        branch path).

        Note that tag paths can also be matched: assuming tags/tag-1.1
        is a tag then:
        tags/tag-1.1 => ('', '../tags/tag-1.1', 'tags/tag-1.1')
        tags/tag-1.1/file => ('file', '../tags/tag-1.1', 'tags/tag-1.1')
        tags/tag-1.2 => (None, None, None)

        If existing=True, will return None, None, None if the file isn't on
        some known branch. If existing=False, then it will guess what the
        branch would be if it were known. Server-side branch path should be
        relative to our subdirectory.
        """
        path = self.normalize(path)
        tag = self.get_path_tag(path)
        if tag:
            # consider the new tags when dispatching entries
            matched = []
            for tags in (self.tags, self.addedtags):
                matched += [t for t in tags
                            if (tag == t or tag.startswith(t + '/'))]
            if not matched:
                return None, None, None
            matched.sort(key=len, reverse=True)
            if tag == matched[0]:
                brpath = ''
                svrpath = path
            else:
                brpath = tag[len(matched[0])+1:]
                svrpath = path[:-(len(brpath)+1)]
            ln = self.localname(svrpath)
            return brpath, ln, svrpath

        branch_path, local_path = self.layoutobj.split_remote_name(path,
                                                                   self.branches)
        branch_name = self.layoutobj.localname(branch_path)

        if branch_name in self.branches:
            return local_path, branch_name, branch_path
        elif existing or (branch_name and branch_name.startswith('../')):
            return None, None, None
        else:
            return local_path, branch_name, branch_path

    def _determine_parent_branch(self, p, src_path, src_rev, revnum):
        if src_path is not None:
            src_file, src_branch = self.split_branch_path(src_path)[:2]
            src_tag = self.get_path_tag(src_path)
            if src_tag or src_file == '':
                brpath, fpath = self.layoutobj.split_remote_name(p,
                                                                 self.branches)
                # we'll sometimes get a different path out of
                # split_remate_name than the one we passed in, but
                # only for the root of a branch, since the svn copies
                # of those will sometimes be of parent directories of
                # our root
                if fpath == '':
                    ln = self.localname(brpath)
                else:
                    ln = self.localname(p)
                if src_tag in self.tags:
                    changeid = self.tags[src_tag]
                    src_rev, src_branch = self.get_source_rev(changeid)[:2]
                return {ln: (src_branch, src_rev, revnum)}
        return {}

    def is_path_valid(self, path, existing=True):
        if path is None:
            return False
        subpath = self.split_branch_path(path, existing)[0]
        if subpath is None:
            return False
        return subpath in self.filemap

    def get_parent_svn_branch_and_rev(self, number, branch, exact=False):
        """Return the parent revision of branch at number as a tuple
        (parentnum, parentbranch) or (None, None) if undefined.

        By default, current revision copy records will be used to resolve
        the parent. For instance, if branch1 is replaced by branch2 in
        current revision, then the parent of current revision on branch1
        will be branch2. In this case, use exact=True to select the
        existing branch before looking at the copy records.
        """
        if (number, branch) in self.revmap:
            return number, branch
        real_num = 0
        for num, br in self.revmap.iterkeys():
            if br != branch:
                continue
            if num <= number and num > real_num:
                real_num = num
        if branch in self.branches:
            parent_branch = self.branches[branch][0]
            parent_branch_rev = self.branches[branch][1]
            # check to see if this branch already existed and is the same
            if parent_branch_rev < real_num:
                return real_num, branch
            # if that wasn't true, then this is the a new branch with the
            # same name as some old deleted branch
            if parent_branch_rev <= 0 and real_num == 0:
                return None, None
            branch_created_rev = self.branches[branch][2]
            if parent_branch == 'trunk':
                parent_branch = None
            if branch_created_rev <= number + 1 and branch != parent_branch:
                # did the branch exist in previous run
                if exact and branch in self.prevbranches:
                    if self.prevbranches[branch][1] < real_num:
                        return real_num, branch
                return self.get_parent_svn_branch_and_rev(
                    parent_branch_rev, parent_branch)
        if real_num != 0:
            return real_num, branch
        return None, None

    def get_parent_revision(self, number, branch, exact=False):
        '''Get the parent revision hash for a commit on a specific branch.
        '''
        tag = self.get_path_tag(self.remotename(branch))
        if tag:
            # Reference a tag being created
            if tag in self.addedtags:
                tbranch, trev = self.addedtags[tag]
                fromtag = self.get_path_tag(self.remotename(tbranch))
                if not fromtag:
                    # Created from a regular branch, not another tag
                    tagged = self.get_parent_svn_branch_and_rev(trev, tbranch)
                    return node.hex(self.revmap[tagged])
                tag = fromtag
            # Reference an existing tag
            limitedtags = maps.Tags(self.repo, endrev=number - 1)
            if tag in limitedtags:
                return limitedtags[tag]
        r, br = self.get_parent_svn_branch_and_rev(number - 1, branch, exact)
        if r is not None:
            return self.revmap[r, br]
        return revlog.nullid

    def get_source_rev(self, changeid=None, ctx=None):
        """Return the source svn revision, the branch name and the svn
        branch path or a converted changeset. If supplied revision
        has no conversion record, raise KeyError.

        If ctx is None, build one from supplied changeid
        """
        if ctx is None:
            ctx = self.repo[changeid]
        extra = ctx.extra()
        if 'convert_revision' not in extra:
            raise KeyError('%s has no conversion record' % ctx)
        branchpath, revnum = extra['convert_revision'][40:].rsplit('@', 1)
        branch = self.localname(self.normalize(branchpath))
        if branchpath and branchpath[0] == '/':
            branchpath = branchpath[1:]
        return int(revnum), branch, branchpath

    def update_branch_tag_map_for_rev(self, revision):
        """Given a revision object, determine changes to branches.

        Returns: a dict of {
            'branches': (added_branches, self.closebranches),
        } where adds are dicts where the keys are branch names and
        values are the place the branch came from. The deletions are
        sets of the deleted branches.
        """
        paths = revision.paths
        added_branches = {}
        # Reset the tags delta before detecting the new one, and take
        # care not to fill them until done since split_branch_path()
        # use them.
        self.addedtags, self.deletedtags = {}, {}
        addedtags, deletedtags = {}, {}
        self.closebranches = set()
        for p in sorted(paths):
            t_name = self.get_path_tag(p)
            if t_name:
                src_p, src_rev = paths[p].copyfrom_path, paths[p].copyfrom_rev
                if src_p is not None and src_rev is not None:
                    file, branch = self.split_branch_path(src_p)[:2]
                    from_tag = self.get_path_tag(src_p)
                    if file is None and not from_tag:
                        continue
                    if from_tag and from_tag not in self.tags:
                        # Ignore copies from unknown tags
                        continue
                    if not file:
                        # Direct branch or tag copy
                        if from_tag:
                            changeid = self.tags[from_tag]
                            src_rev, branch = self.get_source_rev(changeid)[:2]
                        if t_name not in addedtags:
                            addedtags[t_name] = branch, src_rev
                    else:
                        # Subbranch or subtag copy
                        t_name = t_name[:-(len(file)+1)]
                        found = t_name in addedtags
                        if found and src_rev > addedtags[t_name][1]:
                            addedtags[t_name] = branch, src_rev
                elif (paths[p].action == 'D' and p.endswith(t_name)
                      and t_name in self.tags):
                    branch = self.get_source_rev(self.tags[t_name])[1]
                    deletedtags[t_name] = branch, None
                continue

            # At this point we know the path is not a tag. In that
            # case, we only care if it is the root of a new branch (in
            # this function). This is determined by the following
            # checks:
            # 1. Is the file located inside any currently known
            #    branch?  If yes, then we're done with it, this isn't
            #    interesting.
            # 2. Does the file have copyfrom information? If yes, and
            #    the branch is being replaced by what would be an
            #    ancestor, treat it as a regular revert. Otherwise,
            #    we're done: this is a new branch, and we record the
            #    copyfrom in added_branches if it comes from the root
            #    of another branch, or create it from scratch.
            # 3. Neither of the above. This could be a branch, but it
            #    might never work out for us. It's only ever a branch
            #    (as far as we're concerned) if it gets committed to,
            #    which we have to detect at file-write time anyway. So
            #    we do nothing here.
            # 4. It's the root of an already-known branch, with an
            #    action of 'D'. We mark the branch as deleted.
            # 5. It's the parent directory of one or more
            #    already-known branches, so we mark them as deleted.
            # 6. It's a branch being replaced by another branch or a new
            #    directory - the action will be 'R'.
            fi, br = self.split_branch_path(p)[:2]
            if fi is not None:
                if fi == '':
                    if paths[p].action == 'D':
                        self.closebranches.add(br) # case 4
                    elif paths[p].action == 'R':
                        # Check the replacing source is not an ancestor
                        # branch of the branch being replaced, this
                        # would just be a revert.
                        if paths[p].copyfrom_path:
                            cfi, cbr = self.split_branch_path(
                                paths[p].copyfrom_path, paths[p].copyfrom_rev)[:2]
                            if cfi == '':
                                cctx = self.repo[self.get_parent_revision(
                                    paths[p].copyfrom_rev + 1, cbr)]
                                ctx = self.repo[self.get_parent_revision(
                                    revision.revnum, br)]
                                if cctx and util.isancestor(ctx, cctx):
                                    continue
                        parent = self._determine_parent_branch(
                            p, paths[p].copyfrom_path, paths[p].copyfrom_rev,
                            revision.revnum)
                        added_branches.update(parent)
                continue # case 1
            if paths[p].action == 'D':
                for known in self.branches:
                    if self.remotename(known).startswith(p):
                        self.closebranches.add(known) # case 5
            parent = self._determine_parent_branch(
                p, paths[p].copyfrom_path, paths[p].copyfrom_rev,
                revision.revnum)
            if not parent and paths[p].copyfrom_path:
                bpath, branch = self.split_branch_path(p, False)[:2]
                if (bpath is not None
                    and branch not in self.branches
                    and branch not in added_branches):
                    parent = {branch: (None, 0, revision.revnum)}
                elif bpath is None:
                    srcpath = paths[p].copyfrom_path
                    srcrev = paths[p].copyfrom_rev
                    parent = {}
                    for br in self.branches:
                        rn = self.remotename(br)
                        if rn.startswith(srcpath[1:] + '/'):
                            bname = posixpath.basename(rn)
                            newbr = posixpath.join(p, bname)
                            parent.update(
                                self._determine_parent_branch(
                                    newbr, rn, srcrev, revision.revnum))
            added_branches.update(parent)
        self.addedtags, self.deletedtags = addedtags, deletedtags
        return {
            'branches': (added_branches, self.closebranches),
        }

    def save_tbdelta(self, tbdelta):
        self.prevbranches = dict(self.branches)
        for br in tbdelta['branches'][1]:
            del self.branches[br]
        self.branches.update(tbdelta['branches'][0])

    def movetag(self, tag, hash, rev, date):
        if tag in self.tags and self.tags[tag] == hash:
            return

        # determine branch from earliest unclosed ancestor
        branchparent = self.repo[hash]
        while branchparent.extra().get('close'):
            branchparent = branchparent.parents()[0]
        branch = self.get_source_rev(ctx=branchparent)[1]

        parentctx = self.repo[self.get_parent_revision(rev.revnum + 1, branch)]
        if '.hgtags' in parentctx:
            tagdata = parentctx.filectx('.hgtags').data()
        else:
            tagdata = ''
        tagdata += '%s %s\n' % (node.hex(hash), self.tagmap.get(tag, tag))
        def hgtagsfn(repo, memctx, path):
            assert path == '.hgtags'
            return context.memfilectx(path=path,
                                      data=tagdata,
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        revnum, branch = self.get_source_rev(ctx=parentctx)[:2]
        newparent = None
        for child in parentctx.children():
            if (self.get_source_rev(ctx=child)[1] == branch
                and child.extra().get('close', False)):
                newparent = child
        if newparent:
            parentctx = newparent
            revnum, branch = self.get_source_rev(ctx=parentctx)[:2]
        ctx = context.memctx(self.repo,
                             (parentctx.node(), node.nullid),
                             util.getmessage(self.ui, rev),
                             ['.hgtags', ],
                             hgtagsfn,
                             self.authors[rev.author],
                             date,
                             parentctx.extra())
        new_hash = self.repo.svn_commitctx(ctx)
        if not newparent:
            assert self.revmap[revnum, branch] == parentctx.node()
            self.revmap[revnum, branch] = new_hash
        self.tags[tag] = hash, rev.revnum
        util.describe_commit(self.ui, new_hash, branch)

    def committags(self, rev, endbranches):
        if not self.addedtags and not self.deletedtags:
            return
        date = self.fixdate(rev.date)
        # determine additions/deletions per branch
        branches = {}
        for tags in (self.addedtags, self.deletedtags):
            for tag, (branch, srcrev) in tags.iteritems():
                op = srcrev is None and 'rm' or 'add'
                branches.setdefault(branch, []).append((op, tag, srcrev))

        for b, tags in branches.iteritems():

            # modify parent's .hgtags source

            parent = self.repo[self.get_parent_revision(rev.revnum, b)]
            if '.hgtags' not in parent:
                src = ''
            else:
                src = parent['.hgtags'].data()

            fromtag = self.get_path_tag(self.remotename(b))
            for op, tag, r in sorted(tags, reverse=True):

                if tag in self.tagmap and not self.tagmap[tag]:
                    continue

                tagged = node.hex(node.nullid) # op != 'add'
                if op == 'add':
                    if fromtag:
                        if fromtag in self.tags:
                            tagged = node.hex(self.tags[fromtag])
                    else:
                        tagged = node.hex(self.revmap[
                            self.get_parent_svn_branch_and_rev(r, b)])

                src += '%s %s\n' % (tagged, self.tagmap.get(tag, tag))
                self.tags[tag] = node.bin(tagged), rev.revnum

            # add new changeset containing updated .hgtags
            def fctxfun(repo, memctx, path):
                return context.memfilectx(path='.hgtags', data=src,
                                          islink=False, isexec=False,
                                          copied=None)

            extra = self.genextra(rev.revnum, b)
            if fromtag:
                extra['branch'] = parent.extra().get('branch', 'default')
            self.mapbranch(extra, b in endbranches or fromtag)

            ctx = context.memctx(self.repo,
                                 (parent.node(), node.nullid),
                                 util.getmessage(self.ui, rev),
                                 ['.hgtags'],
                                 fctxfun,
                                 self.authors[rev.author],
                                 date,
                                 extra)
            new = self.repo.svn_commitctx(ctx)

            if not fromtag and (rev.revnum, b) not in self.revmap:
                self.revmap[rev.revnum, b] = new
            if b in endbranches:
                endbranches.pop(b)
                bname = b or 'default'
                self.ui.status('Marked branch %s as closed.\n' % bname)

    def delbranch(self, branch, node, rev):
        pctx = self.repo[node]
        files = pctx.manifest().keys()
        extra = self.genextra(rev.revnum, branch)
        self.mapbranch(extra, True)
        ctx = context.memctx(self.repo,
                             (node, revlog.nullid),
                             util.getmessage(self.ui, rev),
                             [],
                             lambda x, y, z: None,
                             self.authors[rev.author],
                             self.fixdate(rev.date),
                             extra)
        new = self.repo.svn_commitctx(ctx)
        self.ui.status('Marked branch %s as closed.\n' % (branch or 'default'))
