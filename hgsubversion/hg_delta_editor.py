import cStringIO
import cPickle as pickle
import os
import sys
import tempfile
import traceback

from mercurial import context
from mercurial import hg
from mercurial import ui
from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node
from mercurial import error
from svn import delta
from svn import core

import svnexternals
import util
import maps

class MissingPlainTextError(Exception):
    """Exception raised when the repo lacks a source file required for replaying
    a txdelta.
    """

class ReplayException(Exception):
    """Exception raised when you try and commit but the replay encountered an
    exception.
    """

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

def ieditor(fn):
    """Helps identify methods used by the SVN editor interface.

    Stash any exception raised in the method on self.

    This is required because the SWIG bindings just mutate any exception into
    a generic Subversion exception with no way of telling what the original was.
    This allows the editor object to notice when you try and commit and really
    got an exception in the replay process.
    """
    def fun(self, *args, **kwargs):
        try:
            return fn(self, *args, **kwargs)
        except: #pragma: no cover
            if not hasattr(self, '_exception_info'):
                self._exception_info = sys.exc_info()
            raise
    return fun


class HgChangeReceiver(delta.Editor):
    def add_to_revmap(self, revnum, branch, node_hash):
        f = open(self.revmap_file, 'a')
        f.write(str(revnum) + ' ' + node.hex(node_hash) + ' ' + (branch or '') + '\n')
        f.flush()
        f.close()
        self.revmap[revnum, branch] = node_hash

    def last_known_revision(self):
        """Obtain the highest numbered -- i.e. latest -- revision known.

        Currently, this function just iterates over the entire revision map
        using the max() builtin. This may be slow for extremely large
        repositories, but for now, it's fast enough.
        """
        try:
            return max(k[0] for k in self.revmap.iterkeys())
        except ValueError:
            return 0

    def __init__(self, repo=None, path=None, ui_=None,
                 subdir='', author_host='',
                 tag_locations=[],
                 authors=None, filemap=None, uuid=None):
        """path is the path to the target hg repo.

        subdir is the subdirectory of the edits *on the svn server*.
        It is needed for stripping paths off in certain cases.
        """
        if repo and repo.ui and not ui_:
            ui_ = repo.ui
        if not ui_:
            ui_ = ui.ui()
        self.ui = ui_
        self.__setup_repo(uuid, repo, path, subdir)

        if not author_host:
            author_host = self.ui.config('hgsubversion', 'defaulthost', uuid)
        if not authors:
            authors = self.ui.config('hgsubversion', 'authormap')
        if not filemap:
            filemap = self.ui.config('hgsubversion', 'filemap')
        if not tag_locations:
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
        if os.path.exists(self.tag_info_file):
            f = open(self.tag_info_file)
            self.tags = pickle.load(f)
            f.close()
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

        self.clear_current_info()
        self.authors = maps.AuthorMap(self.ui, self.authors_file,
                                 defaulthost=author_host)
        if authors: self.authors.load(authors)

        self.lastdate = '1970-01-01 00:00:00 -0000'
        self.includepaths = {}
        self.excludepaths = {}
        if filemap and os.path.exists(filemap):
            self.readfilemap(filemap)

    def fixdate(self, date):
        if date is not None:
            date = date.replace('T', ' ').replace('Z', '').split('.')[0]
            date += ' -0000'
            self.lastdate = date
        else:
            date = self.lastdate
        return date

    def __setup_repo(self, uuid, repo, path, subdir):
        """Verify the repo is going to work out for us.

        This method will fail an assertion if the repo exists but doesn't have
        the Subversion metadata.
        """
        if repo:
            self.repo = repo
            self.path = os.path.normpath(self.repo.join('..'))
        elif path:
            self.repo = hg.repository(self.ui, path,
                                      create=(not os.path.exists(path)))
            self.path = os.path.normpath(os.path.join(path, '..'))
        else: #pragma: no cover
            raise TypeError("editor requires either a path or a repository "
                            "specified")

        if not os.path.isdir(self.meta_data_dir):
            os.makedirs(self.meta_data_dir)
        self._set_uuid(uuid)
        # TODO: validate subdir too

        if os.path.isfile(self.revmap_file):
            self.revmap = util.parse_revmap(self.revmap_file)
        else:
            self.revmap = {}
            f = open(self.revmap_file, 'w')
            f.write('%s\n' % util.REVMAP_FILE_VERSION)
            f.flush()
            f.close()

    def clear_current_info(self):
        '''Clear the info relevant to a replayed revision so that the next
        revision can be replayed.
        '''
        # Map files to raw svn data (symlink prefix is preserved)
        self.current_files = {}
        self.deleted_files = {}
        self.current_rev = None
        self.current_files_exec = {}
        self.current_files_symlink = {}
        self.dir_batons = {}
        # Map fully qualified destination file paths to module source path
        self.copies = {}
        self.missing_plaintexts = set()
        self.commit_branches_empty = {}
        self.base_revision = None
        self.branches_to_delete = set()
        self.externals = {}

    def _save_metadata(self):
        '''Save the Subversion metadata. This should really be called after
        every revision is created.
        '''
        pickle_atomic(self.branches, self.branch_info_file, self.meta_data_dir)
        pickle_atomic(self.tags, self.tag_info_file, self.meta_data_dir)

    def _path_and_branch_for_path(self, path, existing=True):
        return self._split_branch_path(path, existing=existing)[:2]

    def _branch_for_path(self, path, existing=True):
        return self._path_and_branch_for_path(path, existing=existing)[1]

    def _localname(self, path):
        """Compute the local name for a branch located at path.
        """
        assert not path.startswith('tags/')
        if path == 'trunk':
            return None
        elif path.startswith('branches/'):
            return path[len('branches/'):]
        return  '../%s' % path

    def _remotename(self, branch):
        if branch == 'default' or branch is None:
            return 'trunk'
        elif branch.startswith('../'):
            return branch[3:]
        return 'branches/%s' % branch

    def _split_branch_path(self, path, existing=True):
        """Figure out which branch inside our repo this path represents, and
        also figure out which path inside that branch it is.

        Returns a tuple of (path within branch, local branch name, server-side branch path).

        If existing=True, will return None, None, None if the file isn't on some known
        branch. If existing=False, then it will guess what the branch would be if it were
        known.
        """
        path = self._normalize_path(path)
        if path.startswith('tags/'):
            return None, None, None
        test = ''
        path_comps = path.split('/')
        while self._localname(test) not in self.branches and len(path_comps):
            if not test:
                test = path_comps.pop(0)
            else:
                test += '/%s' % path_comps.pop(0)
        if self._localname(test) in self.branches:
            return path[len(test)+1:], self._localname(test), test
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
        ln =  self._localname(test)
        if ln and ln.startswith('../'):
            return None, None, None
        return path, ln, test

    def set_current_rev(self, rev):
        """Set the revision we're currently converting.
        """
        self.current_rev = rev

    def set_file(self, path, data, isexec=False, islink=False):
        if islink:
            data = 'link ' + data
        self.current_files[path] = data
        self.current_files_exec[path] = isexec
        self.current_files_symlink[path] = islink
        if path in self.deleted_files:
            del self.deleted_files[path]
        if path in self.missing_plaintexts:
            self.missing_plaintexts.remove(path)

    def delete_file(self, path):
        self.deleted_files[path] = True
        if path in self.current_files:
            del self.current_files[path]
        self.current_files_exec[path] = False
        self.current_files_symlink[path] = False
        self.ui.note('D %s\n' % path)

    def _normalize_path(self, path):
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

    def _is_file_included(self, subpath):
        def checkpathinmap(path, mapping):
            def rpairs(name):
                yield '.', name
                e = len(name)
                while e != -1:
                    yield name[:e], name[e+1:]
                    e = name.rfind('/', 0, e)

            for pre, suf in rpairs(path):
                try:
                    return mapping[pre]
                except KeyError, err:
                    pass
            return None

        if len(self.includepaths) and len(subpath):
            inc = checkpathinmap(subpath, self.includepaths)
        else:
            inc = subpath
        if len(self.excludepaths) and len(subpath):
            exc = checkpathinmap(subpath, self.excludepaths)
        else:
            exc = None
        if inc is None or exc is not None:
            return False
        return True

    def _is_path_valid(self, path):
        if path is None:
            return False
        subpath = self._split_branch_path(path)[0]
        if subpath is None:
            return False
        return self._is_file_included(subpath)

    def _is_path_tag(self, path):
        """If path could represent the path to a tag, returns the potential tag
        name. Otherwise, returns False.

        Note that it's only a tag if it was copied from the path '' in a branch
        (or tag) we have, for our purposes.
        """
        path = self._normalize_path(path)
        for tagspath in self.tag_locations:
            onpath = path.startswith(tagspath)
            longer = len(path) > len('%s/' % tagspath)
            if path and onpath and longer:
                tag, subpath = path[len(tagspath) + 1:], ''
                return tag
        return False

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

    def _svnpath(self, branch):
        """Return the relative path in svn of branch.
        """
        if branch == None or branch == 'default':
            return 'trunk'
        elif branch.startswith('../'):
            return branch[3:]
        return 'branches/%s' % branch

    def _determine_parent_branch(self, p, src_path, src_rev, revnum):
        if src_path is not None:
            src_file, src_branch = self._path_and_branch_for_path(src_path)
            src_tag = self._is_path_tag(src_path)
            if src_tag != False:
                # also case 2
                src_branch, src_rev = self.tags[src_tag]
                return {self._localname(p): (src_branch, src_rev, revnum )}
            if src_file == '':
                # case 2
                return {self._localname(p): (src_branch, src_rev, revnum )}
        return {}

    def update_branch_tag_map_for_rev(self, revision):
        paths = revision.paths
        added_branches = {}
        added_tags = {}
        self.branches_to_delete = set()
        tags_to_delete = set()
        for p in sorted(paths):
            t_name = self._is_path_tag(p)
            if t_name != False:
                src_p, src_rev = paths[p].copyfrom_path, paths[p].copyfrom_rev
                # if you commit to a tag, I'm calling you stupid and ignoring
                # you.
                if src_p is not None and src_rev is not None:
                    file, branch = self._path_and_branch_for_path(src_p)
                    if file is None:
                        # some crazy people make tags from other tags
                        file = ''
                        from_tag = self._is_path_tag(src_p)
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
            fi, br = self._path_and_branch_for_path(p)
            if fi is not None:
                if fi == '':
                    if paths[p].action == 'D':
                        self.branches_to_delete.add(br) # case 4
                    elif paths[p].action == 'R':
                        parent = self._determine_parent_branch(
                            p, paths[p].copyfrom_path, paths[p].copyfrom_rev,
                            revision.revnum)
                        added_branches.update(parent)
                continue # case 1
            if paths[p].action == 'D':
                for known in self.branches:
                    if self._svnpath(known).startswith(p):
                        self.branches_to_delete.add(known) # case 5
            parent = self._determine_parent_branch(
                p, paths[p].copyfrom_path, paths[p].copyfrom_rev, revision.revnum)
            if not parent and paths[p].copyfrom_path:
                bpath, branch = self._path_and_branch_for_path(p, False)
                if (bpath is not None
                    and branch not in self.branches
                    and branch not in added_branches):
                    parent = {branch: (None, 0, revision.revnum)}
            added_branches.update(parent)
        rmtags = dict((t, self.tags[t][0]) for t in tags_to_delete)
        return {
            'tags': (added_tags, rmtags),
            'branches': (added_branches, self.branches_to_delete),
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

    def _updateexternals(self):
        if not self.externals:
            return
        # Accumulate externals records for all branches
        revnum = self.current_rev.revnum
        branches = {}
        for path, entry in self.externals.iteritems():
            if not self._is_path_valid(path):
                self.ui.warn('WARNING: Invalid path %s in externals\n' % path)
                continue
            p, b, bp = self._split_branch_path(path)
            if bp not in branches:
                external = svnexternals.externalsfile()
                parent = self.get_parent_revision(revnum, b)
                pctx = self.repo[parent]
                if '.hgsvnexternals' in pctx:
                    external.read(pctx['.hgsvnexternals'].data())
                branches[bp] = external
            else:
                external = branches[bp]
            external[p] = entry

        # Register the file changes
        for bp, external in branches.iteritems():
            path = bp + '/.hgsvnexternals'
            if external:
                self.set_file(path, external.write(), False, False)
            else:
                self.delete_file(path)

    def branchedits(self, branch, rev):
        check = lambda x: x[0][1] == branch and x[0][0] < rev.revnum
        return sorted(filter(check, self.revmap.iteritems()), reverse=True)

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
            parent = self.repo[{None: 'default'}.get(b, b)]
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
                self.add_to_revmap(rev.revnum, b, new)
            if b in endbranches:
                endbranches[b] = new

    def commit_current_delta(self, tbdelta):
        if hasattr(self, '_exception_info'):  #pragma: no cover
            traceback.print_exception(*self._exception_info)
            raise ReplayException()
        if self.missing_plaintexts:
            raise MissingPlainTextError()
        self._updateexternals()
        # paranoidly generate the list of files to commit
        files_to_commit = set(self.current_files.keys())
        files_to_commit.update(self.current_files_symlink.keys())
        files_to_commit.update(self.current_files_exec.keys())
        files_to_commit.update(self.deleted_files.keys())
        # back to a list and sort so we get sane behavior
        files_to_commit = list(files_to_commit)
        files_to_commit.sort()
        branch_batches = {}
        rev = self.current_rev
        date = self.fixdate(rev.date)

        # build up the branches that have files on them
        for f in files_to_commit:
            if not  self._is_path_valid(f):
                continue
            p, b = self._path_and_branch_for_path(f)
            if b not in branch_batches:
                branch_batches[b] = []
            branch_batches[b].append((p, f))

        closebranches = {}
        for branch in tbdelta['branches'][1]:
            branchedits = self.branchedits(branch, rev)
            if len(branchedits) < 1:
                # can't close a branch that never existed
                continue
            ha = branchedits[0][1]
            closebranches[branch] = ha

        # 1. handle normal commits
        closedrevs = closebranches.values()
        for branch, files in branch_batches.iteritems():
            if branch in self.commit_branches_empty and files:
                del self.commit_branches_empty[branch]
            files = dict(files)

            parents = (self.get_parent_revision(rev.revnum, branch),
                       revlog.nullid)
            if parents[0] in closedrevs and branch in self.branches_to_delete:
                continue
            extra = util.build_extra(rev.revnum, branch, self.uuid, self.subdir)
            if branch is not None:
                if (branch not in self.branches
                    and branch not in self.repo.branchtags()):
                    continue
            parent_ctx = self.repo.changectx(parents[0])
            if '.hgsvnexternals' not in parent_ctx and '.hgsvnexternals' in files:
                # Do not register empty externals files
                if (files['.hgsvnexternals'] in self.current_files
                    and not self.current_files[files['.hgsvnexternals']]):
                    del files['.hgsvnexternals']

            def filectxfn(repo, memctx, path):
                current_file = files[path]
                if current_file in self.deleted_files:
                    raise IOError()
                copied = self.copies.get(current_file)
                flags = parent_ctx.flags(path)
                is_exec = self.current_files_exec.get(current_file, 'x' in flags)
                is_link = self.current_files_symlink.get(current_file, 'l' in flags)
                if current_file in self.current_files:
                    data = self.current_files[current_file]
                    if is_link and data.startswith('link '):
                        data = data[len('link '):]
                    elif is_link:
                        self.ui.warn('file marked as link, but contains data: '
                                     '%s (%r)\n' % (current_file, flags))
                else:
                    data = parent_ctx.filectx(path).data()
                return context.memfilectx(path=path,
                                          data=data,
                                          islink=is_link, isexec=is_exec,
                                          copied=copied)
            if not self.usebranchnames:
                extra.pop('branch', None)
            current_ctx = context.memctx(self.repo,
                                         parents,
                                         rev.message or '...',
                                         files.keys(),
                                         filectxfn,
                                         self.authors[rev.author],
                                         date,
                                         extra)
            new_hash = self.repo.commitctx(current_ctx)
            util.describe_commit(self.ui, new_hash, branch)
            if (rev.revnum, branch) not in self.revmap:
                self.add_to_revmap(rev.revnum, branch, new_hash)

        # 2. handle branches that need to be committed without any files
        for branch in self.commit_branches_empty:
            ha = self.get_parent_revision(rev.revnum, branch)
            if ha == node.nullid:
                continue
            parent_ctx = self.repo.changectx(ha)
            def del_all_files(*args):
                raise IOError
            # True here meant nuke all files, shouldn't happen with branch closing
            if self.commit_branches_empty[branch]: #pragma: no cover
               raise hgutil.Abort('Empty commit to an open branch attempted. '
                                  'Please report this issue.')
            extra = util.build_extra(rev.revnum, branch, self.uuid, self.subdir)
            if not self.usebranchnames:
                extra.pop('branch', None)
            current_ctx = context.memctx(self.repo,
                                         (ha, node.nullid),
                                         rev.message or ' ',
                                         [],
                                         del_all_files,
                                         self.authors[rev.author],
                                         date,
                                         extra)
            new_hash = self.repo.commitctx(current_ctx)
            util.describe_commit(self.ui, new_hash, branch)
            if (rev.revnum, branch) not in self.revmap:
                self.add_to_revmap(rev.revnum, branch, new_hash)

        # 3. handle tags
        if tbdelta['tags'][0] or tbdelta['tags'][1]:
            self.committags(tbdelta['tags'], rev, closebranches)

        # 4. close any branches that need it
        for branch in tbdelta['branches'][1]:
            # self.get_parent_revision(rev.revnum, branch)
            ha = closebranches.get(branch)
            if ha is None:
                continue
            self.delbranch(branch, ha, rev)

        self._save_metadata()
        self.clear_current_info()

    def delbranch(self, branch, node, rev):
        pctx = self.repo[node]
        def filectxfun(repo, memctx, path):
            return pctx[path]
        files = pctx.manifest().keys()
        extra = {'close': 1}
        if self.usebranchnames:
            extra['branch'] = branch or 'default'
        ctx = context.memctx(self.repo,
                             (node, revlog.nullid),
                             rev.message or util.default_commit_msg,
                             files,
                             filectxfun,
                             self.authors[rev.author],
                             self.fixdate(rev.date),
                             extra)
        new = self.repo.commitctx(ctx)
        self.ui.status('Marked branch %s as closed.\n' % (branch or 'default'))

    def readfilemap(self, filemapfile):
        self.ui.note(
            ('Reading file map from %s\n')
            % filemapfile)
        def addpathtomap(path, mapping, mapname):
            if path in mapping:
                self.ui.warn(('Duplicate %s entry in %s: "%d"\n') %
                             (mapname, filemapfile, path))
            else:
                self.ui.debug(('%sing %s\n') %
                              (mapname.capitalize().strip('e'), path))
                mapping[path] = path

        f = open(filemapfile, 'r')
        for line in f:
            if line.strip() == '' or line.strip()[0] == '#':
                continue
            try:
                cmd, path = line.split(' ', 1)
                cmd = cmd.strip()
                path = path.strip()
                if cmd == 'include':
                    addpathtomap(path, self.includepaths, 'include')
                elif cmd == 'exclude':
                    addpathtomap(path, self.excludepaths, 'exclude')
                else:
                    self.ui.warn(
                        ('Unknown filemap command %s\n')
                        % cmd)
            except IndexError:
                self.ui.warn(
                    ('Ignoring bad line in filemap %s: %s\n')
                    % (filemapfile, line.rstrip()))
        f.close()

    def meta_data_dir(self):
        return os.path.join(self.path, '.hg', 'svn')
    meta_data_dir = property(meta_data_dir)

    def meta_file_named(self, name):
        return os.path.join(self.meta_data_dir, name)

    def revmap_file(self):
        return self.meta_file_named('rev_map')
    revmap_file = property(revmap_file)

    def _get_uuid(self):
        return open(self.meta_file_named('uuid')).read()

    def _set_uuid(self, uuid):
        if not uuid:
            return self._get_uuid()
        elif os.path.isfile(self.meta_file_named('uuid')):
            stored_uuid = self._get_uuid()
            assert stored_uuid
            if uuid != stored_uuid:
                raise hgutil.Abort('unable to operate on unrelated repository')
            else:
                return stored_uuid
        else:
            if uuid:
                f = open(self.meta_file_named('uuid'), 'w')
                f.write(uuid)
                f.flush()
                f.close()
                return self._get_uuid()
            else:
                raise hgutil.Abort('unable to operate on unrelated repository')

    uuid = property(_get_uuid, _set_uuid, None,
                    'Error-checked UUID of source Subversion repository.')

    def branch_info_file(self):
        return self.meta_file_named('branch_info')
    branch_info_file = property(branch_info_file)

    def tag_info_file(self):
        return self.meta_file_named('tag_info')
    tag_info_file = property(tag_info_file)

    def tag_locations_file(self):
        return self.meta_file_named('tag_locations')
    tag_locations_file = property(tag_locations_file)

    def authors_file(self):
        return self.meta_file_named('authors')
    authors_file = property(authors_file)

    def aresamefiles(self, parentctx, childctx, files):
        """Assuming all files exist in childctx and parentctx, return True
        if none of them was changed in-between.
        """
        if parentctx == childctx:
            return True
        if parentctx.rev() > childctx.rev():
            parentctx, childctx = childctx, parentctx

        def selfandancestors(selfctx):
            yield selfctx
            for ctx in selfctx.ancestors():
                yield ctx

        files = dict.fromkeys(files)
        for pctx in selfandancestors(childctx):
            if pctx.rev() <= parentctx.rev():
                return True
            for f in pctx.files():
                if f in files:
                    return False
        # parentctx is not an ancestor of childctx, files are unrelated
        return False

    # Here come all the actual editor methods

    @ieditor
    def delete_entry(self, path, revision_bogus, parent_baton, pool=None):
        br_path, branch = self._path_and_branch_for_path(path)
        if br_path == '':
            self.branches_to_delete.add(branch)
        if br_path is not None:
            ha = self.get_parent_revision(self.current_rev.revnum, branch)
            if ha == revlog.nullid:
                return
            ctx = self.repo.changectx(ha)
            if br_path not in ctx:
                br_path2 = ''
                if br_path != '':
                    br_path2 = br_path + '/'
                # assuming it is a directory
                self.externals[path] = None
                map(self.delete_file, [pat for pat in self.current_files.iterkeys()
                                       if pat.startswith(path+'/')])
                for f in ctx.walk(util.PrefixMatch(br_path2)):
                    f_p = '%s/%s' % (path, f[len(br_path2):])
                    if f_p not in self.current_files:
                        self.delete_file(f_p)
            self.delete_file(path)

    @ieditor
    def open_file(self, path, parent_baton, base_revision, p=None):
        self.current_file = None
        fpath, branch = self._path_and_branch_for_path(path)
        if not fpath:
            self.ui.debug('WARNING: Opening non-existant file %s\n' % path)
            return

        self.current_file = path
        self.ui.note('M %s\n' % path)
        if base_revision != -1:
            self.base_revision = base_revision
        else:
            self.base_revision = None

        if self.current_file in self.current_files:
            return

        baserev = base_revision
        if baserev is None or baserev == -1:
            baserev = self.current_rev.revnum - 1
        parent = self.get_parent_revision(baserev + 1, branch)

        ctx = self.repo[parent]
        if not self._is_path_valid(path):
            return

        if fpath not in ctx:
            self.missing_plaintexts.add(svnpath)

        fctx = ctx.filectx(fpath)
        base = fctx.data()
        if 'l' in fctx.flags():
            base = 'link ' + base
        self.set_file(path, base, 'x' in fctx.flags(), 'l' in fctx.flags())

    @ieditor
    def add_file(self, path, parent_baton=None, copyfrom_path=None,
                 copyfrom_revision=None, file_pool=None):
        self.current_file = None
        self.base_revision = None
        if path in self.deleted_files:
            del self.deleted_files[path]
        fpath, branch = self._path_and_branch_for_path(path, existing=False)
        if not fpath:
            return
        if branch not in self.branches:
            # we know this branch will exist now, because it has at least one file. Rock.
            self.branches[branch] = None, 0, self.current_rev.revnum
        self.current_file = path
        if not copyfrom_path:
            self.ui.note('A %s\n' % path)
            self.set_file(path, '', False, False)
            return
        self.ui.note('A+ %s\n' % path)
        (from_file,
         from_branch) = self._path_and_branch_for_path(copyfrom_path)
        if not from_file:
            self.missing_plaintexts.add(path)
            return
        ha = self.get_parent_revision(copyfrom_revision + 1,
                                      from_branch)
        ctx = self.repo.changectx(ha)
        if from_file in ctx:
            fctx = ctx.filectx(from_file)
            flags = fctx.flags()
            self.set_file(path, fctx.data(), 'x' in flags, 'l' in flags)
        if from_branch == branch:
            parentid = self.get_parent_revision(self.current_rev.revnum,
                                                branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                if self.aresamefiles(parentctx, ctx, [from_file]):
                    self.copies[path] = from_file

    @ieditor
    def add_directory(self, path, parent_baton, copyfrom_path,
                      copyfrom_revision, dir_pool=None):
        self.dir_batons[path] = path
        br_path, branch = self._path_and_branch_for_path(path)
        if br_path is not None:
            if not copyfrom_path and not br_path:
                self.commit_branches_empty[branch] = True
            else:
                self.commit_branches_empty[branch] = False
        if br_path is None or not copyfrom_path:
            return path
        if copyfrom_path:
            tag = self._is_path_tag(copyfrom_path)
            if tag not in self.tags:
                tag = None
            if not self._is_path_valid(copyfrom_path) and not tag:
                self.missing_plaintexts.add('%s/' % path)
                return path
        if tag:
            source_branch, source_rev = self.tags[tag]
            cp_f = ''
        else:
            source_rev = copyfrom_revision
            cp_f, source_branch = self._path_and_branch_for_path(copyfrom_path)
            if cp_f == '' and br_path == '':
                assert br_path is not None
                self.branches[branch] = source_branch, source_rev, self.current_rev.revnum
        new_hash = self.get_parent_revision(source_rev + 1,
                                            source_branch)
        if new_hash == node.nullid:
            self.missing_plaintexts.add('%s/' % path)
            return path
        cp_f_ctx = self.repo.changectx(new_hash)
        if cp_f != '/' and cp_f != '':
            cp_f = '%s/' % cp_f
        else:
            cp_f = ''
        copies = {}
        for f in cp_f_ctx:
            if not f.startswith(cp_f):
                continue
            f2 = f[len(cp_f):]
            fctx = cp_f_ctx.filectx(f)
            fp_c = path + '/' + f2
            self.set_file(fp_c, fctx.data(), 'x' in fctx.flags(), 'l' in fctx.flags())
            if fp_c in self.deleted_files:
                del self.deleted_files[fp_c]
            if branch == source_branch:
                copies[fp_c] = f
        if copies:
            # Preserve the directory copy records if no file was changed between
            # the source and destination revisions, or discard it completely.
            parentid = self.get_parent_revision(self.current_rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                if self.aresamefiles(parentctx, cp_f_ctx, copies.values()):
                    self.copies.update(copies)
        return path

    @ieditor
    def change_file_prop(self, file_baton, name, value, pool=None):
        if name == 'svn:executable':
            self.current_files_exec[self.current_file] = bool(value is not None)
        elif name == 'svn:special':
            self.current_files_symlink[self.current_file] = bool(value is not None)

    @ieditor
    def change_dir_prop(self, dir_baton, name, value, pool=None):
        if dir_baton is None:
            return
        path = self.dir_batons[dir_baton]
        if name == 'svn:externals':
            self.externals[path] = value

    @ieditor
    def open_directory(self, path, parent_baton, base_revision, dir_pool=None):
        self.dir_batons[path] = path
        p_, branch = self._path_and_branch_for_path(path)
        if p_ == '':
            self.commit_branches_empty[branch] = False
        return path

    @ieditor
    def close_directory(self, dir_baton, dir_pool=None):
        if dir_baton is not None:
            del self.dir_batons[dir_baton]

    @ieditor
    def apply_textdelta(self, file_baton, base_checksum, pool=None):
        # We know coming in here the file must be one of the following options:
        # 1) Deleted (invalid, fail an assertion)
        # 2) Missing a base text (bail quick since we have to fetch a full plaintext)
        # 3) Has a base text in self.current_files, apply deltas
        base = ''
        if not self._is_path_valid(self.current_file):
            return lambda x: None
        assert self.current_file not in self.deleted_files, (
            'Cannot apply_textdelta to a deleted file: %s' % self.current_file)
        assert (self.current_file in self.current_files
                or self.current_file in self.missing_plaintexts), '%s not found' % self.current_file
        if self.current_file in self.missing_plaintexts:
            return lambda x: None
        base = self.current_files[self.current_file]
        source = cStringIO.StringIO(base)
        target = cStringIO.StringIO()
        self.stream = target

        handler, baton = delta.svn_txdelta_apply(source, target, None)
        if not callable(handler): #pragma: no cover
            raise hgutil.Abort('Error in Subversion bindings: '
                               'cannot call handler!')
        def txdelt_window(window):
            try:
                if not self._is_path_valid(self.current_file):
                    return
                handler(window, baton)
                # window being None means commit this file
                if not window:
                    self.current_files[self.current_file] = target.getvalue()
            except core.SubversionException, e: #pragma: no cover
                if e.apr_err == core.SVN_ERR_INCOMPLETE_DATA:
                    self.missing_plaintexts.add(self.current_file)
                else: #pragma: no cover
                    raise hgutil.Abort(*e.args)
            except: #pragma: no cover
                print len(base), self.current_file
                self._exception_info = sys.exc_info()
                raise
        return txdelt_window
