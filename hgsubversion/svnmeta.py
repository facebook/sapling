import cPickle as pickle
import os
import tempfile

from mercurial import context
from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node

import util
import maps
import editor


def pickle_atomic(data, file_path, dir=None):
    """pickle some data to a path atomically.

    This is present because I kept corrupting my revmap by managing to hit ^C
    during the pickle of that file.
    """
    try:
        f, path = tempfile.mkstemp(prefix='pickling', dir=dir)
        f = os.fdopen(f, 'w')
        pickle.dump(data, f)
        f.close()
    except: #pragma: no cover
        raise
    else:
        hgutil.rename(path, file_path)


class SVNMeta(object):

    def __init__(self, repo, uuid=None, subdir=''):
        """path is the path to the target hg repo.

        subdir is the subdirectory of the edits *on the svn server*.
        It is needed for stripping paths off in certain cases.
        """
        self.ui = repo.ui
        self.repo = repo
        self.path = os.path.normpath(repo.join('..'))

        if not os.path.isdir(self.meta_data_dir):
            os.makedirs(self.meta_data_dir)
        self._set_uuid(uuid)
        # TODO: validate subdir too
        self.revmap = maps.RevMap(repo)

        author_host = self.ui.config('hgsubversion', 'defaulthost', uuid)
        authors = self.ui.config('hgsubversion', 'authormap')
        tag_locations = self.ui.configlist('hgsubversion', 'tagpaths', ['tags'])
        self.usebranchnames = self.ui.configbool('hgsubversion',
                                                  'usebranchnames', True)

        # FIXME: test that this hasn't changed! defer & compare?
        self.subdir = subdir
        if self.subdir and self.subdir[0] == '/':
            self.subdir = self.subdir[1:]
        self.branches = {}
        if os.path.exists(self.branch_info_file):
            f = open(self.branch_info_file)
            self.branches = pickle.load(f)
            f.close()
        self.tags = maps.TagMap(repo)
        if os.path.exists(self.tag_locations_file):
            f = open(self.tag_locations_file)
            self.tag_locations = pickle.load(f)
            f.close()
        else:
            self.tag_locations = tag_locations
        if os.path.exists(self.layoutfile):
            f = open(self.layoutfile)
            self._layout = f.read().strip()
            f.close()
            self.repo.ui.setconfig('hgsubversion', 'layout', self._layout)
        else:
            self._layout = None
        pickle_atomic(self.tag_locations, self.tag_locations_file,
                      self.meta_data_dir)
        # ensure nested paths are handled properly
        self.tag_locations.sort()
        self.tag_locations.reverse()

        self.authors = maps.AuthorMap(self.ui, self.authors_file,
                                 defaulthost=author_host)
        if authors: self.authors.load(authors)

        self.lastdate = '1970-01-01 00:00:00 -0000'
        self.filemap = maps.FileMap(repo)
        self.addedtags = {}
        self.deletedtags = {}

    @property
    def layout(self):
        # this method can't determine the layout, but it needs to be
        # resolved into something other than auto before this ever
        # gets called
        if not self._layout or self._layout == 'auto':
            lo = self.repo.ui.config('hgsubversion', 'layout', default='auto')
            if lo == 'auto':
                raise hgutil.Abort('layout not yet determined')
            self._layout = lo
            f = open(self.layoutfile, 'w')
            f.write(self._layout)
            f.close()
        return self._layout

    @property
    def editor(self):
        if not hasattr(self, '_editor'):
            self._editor = editor.HgEditor(self)
        return self._editor

    def _get_uuid(self):
        return open(os.path.join(self.meta_data_dir, 'uuid')).read()

    def _set_uuid(self, uuid):
        if not uuid:
            return
        elif os.path.isfile(os.path.join(self.meta_data_dir, 'uuid')):
            stored_uuid = self._get_uuid()
            assert stored_uuid
            if uuid != stored_uuid:
                raise hgutil.Abort('unable to operate on unrelated repository')
        else:
            if uuid:
                f = open(os.path.join(self.meta_data_dir, 'uuid'), 'w')
                f.write(uuid)
                f.flush()
                f.close()
            else:
                raise hgutil.Abort('unable to operate on unrelated repository')

    uuid = property(_get_uuid, _set_uuid, None,
                    'Error-checked UUID of source Subversion repository.')

    @property
    def meta_data_dir(self):
        return os.path.join(self.path, '.hg', 'svn')

    @property
    def branch_info_file(self):
        return os.path.join(self.meta_data_dir, 'branch_info')

    @property
    def tag_locations_file(self):
        return os.path.join(self.meta_data_dir, 'tag_locations')

    @property
    def authors_file(self):
        return os.path.join(self.meta_data_dir, 'authors')

    @property
    def layoutfile(self):
        return os.path.join(self.meta_data_dir, 'layout')

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
        pickle_atomic(self.branches, self.branch_info_file, self.meta_data_dir)

    def localname(self, path):
        """Compute the local name for a branch located at path.
        """
        if self.layout == 'single':
            return 'default'
        if path == 'trunk':
            return None
        elif path.startswith('branches/'):
            return path[len('branches/'):]
        return  '../%s' % path

    def remotename(self, branch):
        if self.layout == 'single':
            return ''
        if branch == 'default' or branch is None:
            return 'trunk'
        elif branch.startswith('../'):
            return branch[3:]
        return 'branches/%s' % branch

    def genextra(self, revnum, branch):
        extra = {}
        subdir = self.subdir
        if subdir and subdir[-1] == '/':
            subdir = subdir[:-1]
        if subdir and subdir[0] != '/':
            subdir = '/' + subdir

        if self.layout == 'single':
            path = subdir or '/'
        else:
            branchpath = 'trunk'
            if branch:
                extra['branch'] = branch
                if branch.startswith('../'):
                    branchpath = branch[3:]
                else:
                    branchpath = 'branches/%s' % branch
            path = '%s/%s' % (subdir , branchpath)

        extra['convert_revision'] = 'svn:%(uuid)s%(path)s@%(rev)s' % {
            'uuid': self.uuid,
            'path': path,
            'rev': revnum,
        }
        return extra

    def normalize(self, path):
        '''Normalize a path to strip of leading slashes and our subdir if we
        have one.
        '''
        if self.subdir and path == self.subdir[:-1]:
            return ''
        if path and path[0] == '/':
            path = path[1:]
        if path and path.startswith(self.subdir):
            path = path[len(self.subdir):]
        if path and path[0] == '/':
            path = path[1:]
        return path

    def get_path_tag(self, path):
        """If path could represent the path to a tag, returns the
        potential (non-empty) tag name. Otherwise, returns None

        Note that it's only a tag if it was copied from the path '' in a branch
        (or tag) we have, for our purposes.
        """
        if self.layout != 'single':
            path = self.normalize(path)
            for tagspath in self.tag_locations:
                if path.startswith(tagspath + '/'):
                    tag = path[len(tagspath) + 1:]
                    if tag:
                        return tag
        return None

    def split_branch_path(self, path, existing=True):
        """Figure out which branch inside our repo this path represents, and
        also figure out which path inside that branch it is.

        Returns a tuple of (path within branch, local branch name, server-side branch path).

        If existing=True, will return None, None, None if the file isn't on some known
        branch. If existing=False, then it will guess what the branch would be if it were
        known. Server-side branch path should be relative to our subdirectory.
        """
        path = self.normalize(path)
        if self.layout == 'single':
            return (path, None, '')
        tag = self.get_path_tag(path)
        if tag:
            # consider the new tags when dispatching entries
            matched = []
            for tags in (self.tags, self.addedtags):
                matched += [t for t in tags if tag.startswith(t + '/')]
            if not matched:
                return None, None, None
            matched.sort(key=len, reverse=True)
            brpath = tag[len(matched[0])+1:]
            svrpath = path[:-(len(brpath)+1)]
            ln = self.localname(svrpath)
            return brpath, ln, svrpath
        test = ''
        path_comps = path.split('/')
        while self.localname(test) not in self.branches and len(path_comps):
            if not test:
                test = path_comps.pop(0)
            else:
                test += '/%s' % path_comps.pop(0)
        if self.localname(test) in self.branches:
            return path[len(test)+1:], self.localname(test), test
        if existing:
            return None, None, None
        if path == 'trunk' or path.startswith('trunk/'):
            path = path.split('/')[1:]
            test = 'trunk'
        elif path.startswith('branches/'):
            elts = path.split('/')
            test = '/'.join(elts[:2])
            path = '/'.join(elts[2:])
        else:
            path = test.split('/')[-1]
            test = '/'.join(test.split('/')[:-1])
        ln =  self.localname(test)
        if ln and ln.startswith('../'):
            return None, None, None
        return path, ln, test

    def _determine_parent_branch(self, p, src_path, src_rev, revnum):
        if src_path is not None:
            src_file, src_branch = self.split_branch_path(src_path)[:2]
            src_tag = self.get_path_tag(src_path)
            if src_tag or src_file == '':
                ln = self.localname(p)
                if src_tag in self.tags:
                    changeid = self.tags[src_tag]
                    src_rev, src_branch = self.get_source_rev(changeid)[:2]
                return {ln: (src_branch, src_rev, revnum)}
        return {}

    def is_path_valid(self, path):
        if path is None:
            return False
        subpath = self.split_branch_path(path)[0]
        if subpath is None:
            return False
        return subpath in self.filemap

    def get_parent_svn_branch_and_rev(self, number, branch):
        number -= 1
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
            if branch_created_rev <= number+1 and branch != parent_branch:
                return self.get_parent_svn_branch_and_rev(
                                                parent_branch_rev+1,
                                                parent_branch)
        if real_num != 0:
            return real_num, branch
        return None, None

    def get_parent_revision(self, number, branch):
        '''Get the parent revision hash for a commit on a specific branch.
        '''
        tag = self.get_path_tag(self.remotename(branch))
        if tag:
            limitedtags = maps.TagMap(self.repo, endrev=number-1)
            if tag in limitedtags:
                ha = limitedtags[tag]
                return ha
        r, br = self.get_parent_svn_branch_and_rev(number, branch)
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
        extra = ctx.extra()['convert_revision']        
        branchpath, revnum = extra[40:].rsplit('@', 1)
        branch = self.localname(self.normalize(branchpath))
        if self.layout == 'single':
            branchpath = ''
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
        if self.layout == 'single':
            return {'branches': ({None: (None, 0, -1), }, set()),
                    }
        paths = revision.paths
        added_branches = {}
        self.addedtags = {}
        self.deletedtags = {}
        self.closebranches = set()
        for p in sorted(paths):
            t_name = self.get_path_tag(p)
            if t_name:
                src_p, src_rev = paths[p].copyfrom_path, paths[p].copyfrom_rev
                if src_p is not None and src_rev is not None:
                    file, branch = self.split_branch_path(src_p)[:2]
                    if file is None:
                        # some crazy people make tags from other tags
                        from_tag = self.get_path_tag(src_p)
                        if not from_tag:
                            continue
                        if from_tag in self.tags:
                            changeid = self.tags[from_tag]
                            src_rev, branch = self.get_source_rev(changeid)[:2]
                            file = ''
                    if t_name not in self.addedtags and file is '':
                        self.addedtags[t_name] = branch, src_rev
                    elif file:
                        t_name = t_name[:-(len(file)+1)]
                        found = t_name in self.addedtags
                        if found and src_rev > self.addedtags[t_name][1]:
                            self.addedtags[t_name] = branch, src_rev
                elif (paths[p].action == 'D' and p.endswith(t_name)
                      and t_name in self.tags):
                    branch = self.get_source_rev(self.tags[t_name])[1]
                    self.deletedtags[t_name] = branch, None
                continue

            # At this point we know the path is not a tag. In that
            # case, we only care if it is the root of a new branch (in
            # this function). This is determined by the following
            # checks:
            # 1. Is the file located inside any currently known
            #    branch?  If yes, then we're done with it, this isn't
            #    interesting.
            # 2. Does the file have copyfrom information? If yes, then
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
            # 6. It's a branch being replaced by another branch - the
            #    action will be 'R'.
            fi, br = self.split_branch_path(p)[:2]
            if fi is not None:
                if fi == '':
                    if paths[p].action == 'D':
                        self.closebranches.add(br) # case 4
                    elif paths[p].action == 'R':
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
                p, paths[p].copyfrom_path, paths[p].copyfrom_rev, revision.revnum)
            if not parent and paths[p].copyfrom_path:
                bpath, branch = self.split_branch_path(p, False)[:2]
                if (bpath is not None
                    and branch not in self.branches
                    and branch not in added_branches):
                    parent = {branch: (None, 0, revision.revnum)}
            added_branches.update(parent)
        return {
            'branches': (added_branches, self.closebranches),
        }

    def save_tbdelta(self, tbdelta):
        for br in tbdelta['branches'][1]:
            del self.branches[br]
        self.branches.update(tbdelta['branches'][0])

    def movetag(self, tag, hash, branch, rev, date):
        if self.tags[tag] == hash:
            return
        if branch == 'default':
            branch = None
        parentctx = self.repo[self.get_parent_revision(rev.revnum+1, branch)]
        if '.hgtags' in parentctx:
            tagdata = parentctx.filectx('.hgtags').data()
        else:
            tagdata = ''
        tagdata += '%s %s\n' % (node.hex(hash), tag, )
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
                             rev.message or '...',
                             ['.hgtags', ],
                             hgtagsfn,
                             self.authors[rev.author],
                             date,
                             parentctx.extra())
        new_hash = self.repo.commitctx(ctx)
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
            fromtag = self.get_path_tag(self.remotename(b))
            # modify parent's .hgtags source
            parent = self.repo[self.get_parent_revision(rev.revnum, b)]
            if '.hgtags' not in parent:
                src = ''
            else:
                src = parent['.hgtags'].data()
            for op, tag, r in sorted(tags, reverse=True):
                if op == 'add':
                    if fromtag:
                        if fromtag in self.tags:
                            tagged = node.hex(self.tags[fromtag])
                    else:
                        tagged = node.hex(self.revmap[
                            self.get_parent_svn_branch_and_rev(r+1, b)])
                else:
                    tagged = node.hex(node.nullid)
                src += '%s %s\n' % (tagged, tag)
                self.tags[tag] = node.bin(tagged), rev.revnum

            # add new changeset containing updated .hgtags
            def fctxfun(repo, memctx, path):
                return context.memfilectx(path='.hgtags', data=src,
                                          islink=False, isexec=False,
                                          copied=None)
            extra = self.genextra(rev.revnum, b)
            if fromtag:
                extra['branch'] = parent.extra().get('branch', 'default')
            if not self.usebranchnames:
                extra.pop('branch', None)
            if b in endbranches or fromtag:
                extra['close'] = 1
            ctx = context.memctx(self.repo,
                                 (parent.node(), node.nullid),
                                 rev.message or ' ',
                                 ['.hgtags'],
                                 fctxfun,
                                 self.authors[rev.author],
                                 date,
                                 extra)
            new = self.repo.commitctx(ctx)

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
        extra['close'] = 1
        if self.usebranchnames:
            extra['branch'] = branch or 'default'
        ctx = context.memctx(self.repo,
                             (node, revlog.nullid),
                             rev.message or util.default_commit_msg,
                             [],
                             lambda x, y, z: None,
                             self.authors[rev.author],
                             self.fixdate(rev.date),
                             extra)
        new = self.repo.commitctx(ctx)
        self.ui.status('Marked branch %s as closed.\n' % (branch or 'default'))
