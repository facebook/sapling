import errno
import traceback

from mercurial import revlog
from mercurial import node
from mercurial import context
from mercurial import util as hgutil

import svnexternals
import util


class MissingPlainTextError(Exception):
    """Exception raised when the repo lacks a source file required for replaying
    a txdelta.
    """

class ReplayException(Exception):
    """Exception raised when you try and commit but the replay encountered an
    exception.
    """

def updateexternals(ui, meta, current):
    # TODO fix and re-enable externals for single-directory clones
    if not current.externals or meta.layout == 'single':
        return

    # accumulate externals records for all branches
    revnum = current.rev.revnum
    branches = {}
    for path, entry in current.externals.iteritems():
        if not meta.is_path_valid(path):
            ui.warn('WARNING: Invalid path %s in externals\n' % path)
            continue

        p, b, bp = meta.split_branch_path(path)
        if bp not in branches:
            external = svnexternals.externalsfile()
            parent = meta.get_parent_revision(revnum, b)
            pctx = meta.repo[parent]
            if '.hgsvnexternals' in pctx:
                external.read(pctx['.hgsvnexternals'].data())
            branches[bp] = (external, pctx)
        else:
            external = branches[bp][0]

        external[p] = entry

    # register externals file changes
    for bp, (external, pctx) in branches.iteritems():
        if bp and bp[-1] != '/':
            bp += '/'
        updates = svnexternals.getchanges(ui, meta.repo, pctx, external)
        for fn, data in updates.iteritems():
            path = (bp and bp + fn) or fn
            if data is not None:
                current.set(path, data, False, False)
            else:
                current.delete(path)

def convert_rev(ui, meta, svn, r, tbdelta):

    editor = meta.editor
    editor.current.clear()
    editor.current.rev = r

    if meta.revmap.oldest <= 0:
        # no prior revisions are known, so fetch the entire revision contents
        svn.get_revision(r.revnum, editor)
    else:
        svn.get_replay(r.revnum, editor, meta.revmap.oldest)

    current = editor.current
    current.findmissing(svn)

    updateexternals(ui, meta, current)

    if current.exception is not None:  #pragma: no cover
        traceback.print_exception(*current.exception)
        raise ReplayException()
    if current.missing:
        raise MissingPlainTextError()

    # paranoidly generate the list of files to commit
    files_to_commit = set(current.files.keys())
    files_to_commit.update(current.symlinks.keys())
    files_to_commit.update(current.execfiles.keys())
    files_to_commit.update(current.deleted.keys())
    # back to a list and sort so we get sane behavior
    files_to_commit = list(files_to_commit)
    files_to_commit.sort()
    branch_batches = {}
    rev = current.rev
    date = meta.fixdate(rev.date)

    # build up the branches that have files on them
    for f in files_to_commit:
        if not meta.is_path_valid(f):
            continue
        p, b = meta.split_branch_path(f)[:2]
        if b not in branch_batches:
            branch_batches[b] = []
        branch_batches[b].append((p, f))

    closebranches = {}
    for branch in tbdelta['branches'][1]:
        branchedits = meta.revmap.branchedits(branch, rev)
        if len(branchedits) < 1:
            # can't close a branch that never existed
            continue
        ha = branchedits[0][1]
        closebranches[branch] = ha

    extraempty = (set(tbdelta['branches'][0]) -
                  (set(current.emptybranches) | set(branch_batches.keys())))
    current.emptybranches.update([(x, False) for x in extraempty])

    # 1. handle normal commits
    closedrevs = closebranches.values()
    for branch, files in branch_batches.iteritems():

        if branch in current.emptybranches and files:
            del current.emptybranches[branch]

        files = dict(files)
        parents = meta.get_parent_revision(rev.revnum, branch), revlog.nullid
        if parents[0] in closedrevs and branch in meta.closebranches:
            continue

        extra = meta.genextra(rev.revnum, branch)
        tag = None
        if branch is not None:
            # New regular tag without modifications, it will be committed by
            # svnmeta.committag(), we can skip the whole branch for now
            tag = meta.get_path_tag(meta.remotename(branch))
            if (tag and tag not in meta.tags
                and branch not in meta.branches
                and branch not in meta.repo.branchtags()
                and not files):
                continue

        parentctx = meta.repo.changectx(parents[0])
        if tag:
            if parentctx.node() == node.nullid:
                continue
            extra.update({'branch': parentctx.extra().get('branch', None),
                          'close': 1})

        def filectxfn(repo, memctx, path):
            current_file = files[path]
            if current_file in current.deleted:
                raise IOError(errno.ENOENT, '%s is deleted' % path)
            copied = current.copies.get(current_file)
            flags = parentctx.flags(path)
            is_exec = current.execfiles.get(current_file, 'x' in flags)
            is_link = current.symlinks.get(current_file, 'l' in flags)
            if current_file in current.files:
                data = current.files[current_file]
                if is_link and data.startswith('link '):
                    data = data[len('link '):]
                elif is_link:
                    ui.debug('file marked as link, but may contain data: '
                             '%s (%r)\n' % (current_file, flags))
            else:
                data = parentctx.filectx(path).data()
            return context.memfilectx(path=path,
                                      data=data,
                                      islink=is_link, isexec=is_exec,
                                      copied=copied)

        meta.mapbranch(extra)
        current_ctx = context.memctx(meta.repo,
                                     parents,
                                     rev.message or '...',
                                     files.keys(),
                                     filectxfn,
                                     meta.authors[rev.author],
                                     date,
                                     extra)

        new_hash = meta.repo.commitctx(current_ctx)
        util.describe_commit(ui, new_hash, branch)
        if (rev.revnum, branch) not in meta.revmap and not tag:
            meta.revmap[rev.revnum, branch] = new_hash
        if tag:
            meta.movetag(tag, new_hash, rev, date)
            meta.addedtags.pop(tag, None)

    # 2. handle branches that need to be committed without any files
    for branch in current.emptybranches:

        ha = meta.get_parent_revision(rev.revnum, branch)
        if ha == node.nullid:
            continue

        parent_ctx = meta.repo.changectx(ha)
        def del_all_files(*args):
            raise IOError(errno.ENOENT, 'deleting all files')

        # True here meant nuke all files, shouldn't happen with branch closing
        if current.emptybranches[branch]: #pragma: no cover
            raise hgutil.Abort('Empty commit to an open branch attempted. '
                               'Please report this issue.')

        extra = meta.genextra(rev.revnum, branch)
        meta.mapbranch(extra)

        current_ctx = context.memctx(meta.repo,
                                     (ha, node.nullid),
                                     rev.message or ' ',
                                     [],
                                     del_all_files,
                                     meta.authors[rev.author],
                                     date,
                                     extra)
        new_hash = meta.repo.commitctx(current_ctx)
        util.describe_commit(ui, new_hash, branch)
        if (rev.revnum, branch) not in meta.revmap:
            meta.revmap[rev.revnum, branch] = new_hash

    return closebranches
