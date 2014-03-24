import errno
import traceback

from mercurial import revlog
from mercurial import node
from mercurial import context
from mercurial import util as hgutil

import compathacks
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
            continue

        p, b, bp = meta.split_branch_path(path)
        if bp not in branches:
            parent = meta.get_parent_revision(revnum, b)
            pctx = meta.repo[parent]
            branches[bp] = (svnexternals.parse(ui, pctx), pctx)
        branches[bp][0][p] = entry

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

def convert_rev(ui, meta, svn, r, tbdelta, firstrun):
    try:
        return _convert_rev(ui, meta, svn, r, tbdelta, firstrun)
    finally:
        meta.editor.current.close()

def _convert_rev(ui, meta, svn, r, tbdelta, firstrun):

    editor = meta.editor
    editor.current.clear()
    editor.current.rev = r
    editor.setsvn(svn)

    if firstrun and meta.firstpulled <= 0:
        # We know nothing about this project, so fetch everything before
        # trying to apply deltas.
        ui.debug('replay: fetching full revision\n')
        svn.get_revision(r.revnum, editor)
    else:
        svn.get_replay(r.revnum, editor, meta.firstpulled)
    editor.close()

    current = editor.current

    updateexternals(ui, meta, current)

    if current.exception is not None:  # pragma: no cover
        traceback.print_exception(*current.exception)
        raise ReplayException()

    files_to_commit = current.files()
    branch_batches = {}
    rev = current.rev
    date = meta.fixdate(rev.date)

    # build up the branches that have files on them
    failoninvalid = ui.configbool('hgsubversion',
            'failoninvalidreplayfile', False)
    for f in files_to_commit:
        if not meta.is_path_valid(f):
            if failoninvalid:
                raise hgutil.Abort('file %s should not be in commit list' % f)
            continue
        p, b = meta.split_branch_path(f)[:2]
        if b not in branch_batches:
            branch_batches[b] = []
        if p:
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
                and branch not in compathacks.branchset(meta.repo)
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
            data, isexec, islink, copied = current.pop(current_file)
            if isexec is None or islink is None:
                flags = parentctx.flags(path)
                if isexec is None:
                    isexec = 'x' in flags
                if islink is None:
                    islink = 'l' in flags

            if data is not None:
                if islink:
                    if data.startswith('link '):
                        data = data[len('link '):]
                    else:
                        ui.debug('file marked as link, but may contain data: '
                            '%s\n' % current_file)
            else:
                data = parentctx.filectx(path).data()
            return context.memfilectx(path=path,
                                      data=data,
                                      islink=islink, isexec=isexec,
                                      copied=copied)

        meta.mapbranch(extra)
        current_ctx = context.memctx(meta.repo,
                                     parents,
                                     util.getmessage(ui, rev),
                                     files.keys(),
                                     filectxfn,
                                     meta.authors[rev.author],
                                     date,
                                     extra)

        new_hash = meta.repo.svn_commitctx(current_ctx)
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
        files = []
        def del_all_files(*args):
            raise IOError(errno.ENOENT, 'deleting all files')

        # True here means nuke all files.  This happens when you
        # replace a branch root with an empty directory
        if current.emptybranches[branch]:
            files = meta.repo[ha].files()

        extra = meta.genextra(rev.revnum, branch)
        meta.mapbranch(extra)

        current_ctx = context.memctx(meta.repo,
                                     (ha, node.nullid),
                                     util.getmessage(ui, rev),
                                     files,
                                     del_all_files,
                                     meta.authors[rev.author],
                                     date,
                                     extra)
        new_hash = meta.repo.svn_commitctx(current_ctx)
        util.describe_commit(ui, new_hash, branch)
        if (rev.revnum, branch) not in meta.revmap:
            meta.revmap[rev.revnum, branch] = new_hash

    return closebranches
