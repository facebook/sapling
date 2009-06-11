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
        self.tags = {}
        if os.path.exists(self.tag_locations_file):
            f = open(self.tag_locations_file)
            self.tag_locations = pickle.load(f)
            f.close()
        else:
            self.tag_locations = tag_locations
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
        assert not path.startswith('tags/')
        if path == 'trunk':
            return None
        elif path.startswith('branches/'):
            return path[len('branches/'):]
        return  '../%s' % path

    def remotename(self, branch):
        if branch == 'default' or branch is None:
            return 'trunk'
        elif branch.startswith('../'):
            return branch[3:]
        return 'branches/%s' % branch

    def normalize(self, path):
        '''Normalize a path to strip of leading slashes and our subdir if we
        have one.
        '''
        if path and path[0] == '/':
            path = path[1:]
        if path and path.startswith(self.subdir):
            path = path[len(self.subdir):]
        if path and path[0] == '/':
            path = path[1:]
        return path

    def is_path_tag(self, path):
        """If path could represent the path to a tag, returns the potential tag
        name. Otherwise, returns False.

        Note that it's only a tag if it was copied from the path '' in a branch
        (or tag) we have, for our purposes.
        """
        path = self.normalize(path)
        for tagspath in self.tag_locations:
            onpath = path.startswith(tagspath)
            longer = len(path) > len('%s/' % tagspath)
            if path and onpath and longer:
                tag, subpath = path[len(tagspath) + 1:], ''
                return tag
        return False

    def split_branch_path(self, path, existing=True):
        """Figure out which branch inside our repo this path represents, and
        also figure out which path inside that branch it is.

        Returns a tuple of (path within branch, local branch name, server-side branch path).

        If existing=True, will return None, None, None if the file isn't on some known
        branch. If existing=False, then it will guess what the branch would be if it were
        known.
        """
        path = self.normalize(path)
        if path.startswith('tags/'):
            return None, None, None
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
            src_tag = self.is_path_tag(src_path)
            if src_tag != False or src_file == '': # case 2
                ln = self.localname(p)
                if src_tag != False:
                    src_branch, src_rev = self.tags[src_tag]
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
        r, br = self.get_parent_svn_branch_and_rev(number, branch)
        if r is not None:
            return self.revmap[r, br]
        return revlog.nullid

    def update_branch_tag_map_for_rev(self, revision):
        paths = revision.paths
        added_branches = {}
        added_tags = {}
        self.closebranches = set()
        tags_to_delete = set()
        for p in sorted(paths):
            t_name = self.is_path_tag(p)
            if t_name != False:
                src_p, src_rev = paths[p].copyfrom_path, paths[p].copyfrom_rev
                # if you commit to a tag, I'm calling you stupid and ignoring
                # you.
                if src_p is not None and src_rev is not None:
                    file, branch = self.split_branch_path(src_p)[:2]
                    if file is None:
                        # some crazy people make tags from other tags
                        file = ''
                        from_tag = self.is_path_tag(src_p)
                        if not from_tag:
                            continue
                        branch, src_rev = self.tags[from_tag]
                    if t_name not in added_tags and file is '':
                        added_tags[t_name] = branch, src_rev
                    elif file:
                        t_name = t_name[:-(len(file)+1)]
                        if src_rev > added_tags[t_name][1]:
                            added_tags[t_name] = branch, src_rev
                elif (paths[p].action == 'D' and p.endswith(t_name)
                      and t_name in self.tags):
                        tags_to_delete.add(t_name)
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
                        self.current.closebranches.add(known) # case 5
            parent = self._determine_parent_branch(
                p, paths[p].copyfrom_path, paths[p].copyfrom_rev, revision.revnum)
            if not parent and paths[p].copyfrom_path:
                bpath, branch = self.split_branch_path(p, False)[:2]
                if (bpath is not None
                    and branch not in self.branches
                    and branch not in added_branches):
                    parent = {branch: (None, 0, revision.revnum)}
            added_branches.update(parent)
        rmtags = dict((t, self.tags[t][0]) for t in tags_to_delete)
        return {
            'tags': (added_tags, rmtags),
            'branches': (added_branches, self.closebranches),
        }

    def save_tbdelta(self, tbdelta):
        for t in tbdelta['tags'][1]:
            del self.tags[t]
        for br in tbdelta['branches'][1]:
            del self.branches[br]
        for t, info in tbdelta['tags'][0].items():
            self.ui.status('Tagged %s@%s as %s\n' %
                           (info[0] or 'trunk', info[1], t))
        self.tags.update(tbdelta['tags'][0])
        self.branches.update(tbdelta['branches'][0])

    def committags(self, delta, rev, endbranches):

        date = self.fixdate(rev.date)
        # determine additions/deletions per branch
        branches = {}
        for tag, source in delta[0].iteritems():
            b, r = source
            branches.setdefault(b, []).append(('add', tag, r))
        for tag, branch in delta[1].iteritems():
            branches.setdefault(branch, []).append(('rm', tag, None))

        for b, tags in branches.iteritems():

            # modify parent's .hgtags source
            parent = self.repo[self.get_parent_revision(rev.revnum, b)]
            if '.hgtags' not in parent:
                src = ''
            else:
                src = parent['.hgtags'].data()
            for op, tag, r in sorted(tags, reverse=True):
                if op == 'add':
                    tagged = node.hex(self.revmap[
                        self.get_parent_svn_branch_and_rev(r+1, b)])
                elif op == 'rm':
                    tagged = node.hex(node.nullid)
                src += '%s %s\n' % (tagged, tag)

            # add new changeset containing updated .hgtags
            def fctxfun(repo, memctx, path):
                return context.memfilectx(path='.hgtags', data=src,
                                          islink=False, isexec=False,
                                          copied=None)
            extra = util.build_extra(rev.revnum, b, self.uuid, self.subdir)
            if not self.usebranchnames:
                extra.pop('branch', None)
            if b in endbranches:
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
            if (rev.revnum, b) not in self.revmap:
                self.revmap[rev.revnum, b] = new
            if b in endbranches:
                endbranches.pop(b)
                bname = b or 'default'
                self.ui.status('Marked branch %s as closed.\n' % bname)

    def delbranch(self, branch, node, rev):
        pctx = self.repo[node]
        files = pctx.manifest().keys()
        extra = {'close': 1}
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
