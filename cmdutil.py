#!/usr/bin/python
import re
import os
import urllib

from mercurial import util as hgutil

from svn import core

import util
import svnwrap
import svnexternals


b_re = re.compile(r'^\+\+\+ b\/([^\n]*)', re.MULTILINE)
a_re = re.compile(r'^--- a\/([^\n]*)', re.MULTILINE)
devnull_re = re.compile(r'^([-+]{3}) /dev/null', re.MULTILINE)
header_re = re.compile(r'^diff --git .* b\/(.*)', re.MULTILINE)
newfile_devnull_re = re.compile(r'^--- /dev/null\n\+\+\+ b/([^\n]*)',
                                re.MULTILINE)


class NoFilesException(Exception):
    """Exception raised when you try and commit without files.
    """

def formatrev(rev):
    if rev == -1:
        return '\t(working copy)'
    return '\t(revision %d)' % rev


def filterdiff(diff, oldrev, newrev):
    diff = newfile_devnull_re.sub(r'--- \1\t(revision 0)' '\n'
                                  r'+++ \1\t(working copy)',
                                  diff)
    oldrev = formatrev(oldrev)
    newrev = formatrev(newrev)
    diff = a_re.sub(r'--- \1'+ oldrev, diff)
    diff = b_re.sub(r'+++ \1' + newrev, diff)
    diff = devnull_re.sub(r'\1 /dev/null\t(working copy)', diff)
    diff = header_re.sub(r'Index: \1' + '\n' + ('=' * 67), diff)
    return diff


def parentrev(ui, repo, hge, svn_commit_hashes):
    """Find the svn parent revision of the repo's dirstate.
    """
    workingctx = repo.parents()[0]
    outrev = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes,
                                     workingctx.node())
    if outrev:
        workingctx = repo[outrev[-1]].parents()[0]
    return workingctx


def replay_convert_rev(hg_editor, svn, r):
    hg_editor.set_current_rev(r)
    svn.get_replay(r.revnum, hg_editor)
    i = 1
    if hg_editor.missing_plaintexts:
        hg_editor.ui.debug('Fetching %s files that could not use replay.\n' %
                           len(hg_editor.missing_plaintexts))
        files_to_grab = set()
        rootpath = svn.subdir and svn.subdir[1:] or ''
        for p in hg_editor.missing_plaintexts:
            hg_editor.ui.note('.')
            hg_editor.ui.flush()
            if p[-1] == '/':
                dirpath = p[len(rootpath):]
                files_to_grab.update([dirpath + f for f,k in
                                      svn.list_files(dirpath, r.revnum)
                                      if k == 'f'])
            else:
                files_to_grab.add(p[len(rootpath):])
        hg_editor.ui.note('\nFetching files...\n')
        for p in files_to_grab:
            hg_editor.ui.note('.')
            hg_editor.ui.flush()
            if i % 50 == 0:
                svn.init_ra_and_client()
            i += 1
            data, mode = svn.get_file(p, r.revnum)
            hg_editor.set_file(p, data, 'x' in mode, 'l' in mode)
        hg_editor.missing_plaintexts = set()
        hg_editor.ui.note('\n')
    hg_editor.commit_current_delta()


def _isdir(svn, branchpath, svndir):
    try:
        svn.list_dir('%s/%s' % (branchpath, svndir))
        return True
    except core.SubversionException:
        return False


def _getdirchanges(svn, branchpath, parentctx, ctx, changedfiles, extchanges):
    """Compute directories to add or delete when moving from parentctx
    to ctx, assuming only 'changedfiles' files changed, and 'extchanges'
    external references changed (as returned by svnexternals.diff()).

    Return (added, deleted) where 'added' is the list of all added
    directories and 'deleted' the list of deleted directories.
    Intermediate directories are included: if a/b/c is new and requires
    the addition of a/b and a, those will be listed too. Intermediate
    deleted directories are also listed, but item order of undefined
    in either list.
    """
    def finddirs(path, includeself=False):
        if includeself:
            yield path
        pos = path.rfind('/')
        while pos != -1:
            yield path[:pos]
            pos = path.rfind('/', 0, pos)

    def getctxdirs(ctx, keptdirs, extdirs):
        dirs = {}
        for f in ctx.manifest():
            for d in finddirs(f):
                if d in dirs:
                    break
                if d in keptdirs:
                    dirs[d] = 1
        for extdir in extdirs:
            for d in finddirs(extdir, True):
                dirs[d] = 1
        return dirs

    deleted, added = [], []
    changeddirs = {}
    for f in changedfiles:
        if f in parentctx and f in ctx:
            # Updated files cannot cause directories to be created
            # or removed.
            continue
        for d in finddirs(f):
            changeddirs[d] = 1
    for e in extchanges:
        if not e[1] or not e[2]:
            for d in finddirs(e[0], True):
                changeddirs[d] = 1
    if not changeddirs:
        return added, deleted
    olddirs = getctxdirs(parentctx, changeddirs,
                         [e[0] for e in extchanges if e[1]])
    newdirs = getctxdirs(ctx, changeddirs,
                         [e[0] for e in extchanges if e[2]])

    for d in newdirs:
        if d not in olddirs and not _isdir(svn, branchpath, d):
            added.append(d)

    for d in olddirs:
        if d not in newdirs and _isdir(svn, branchpath, d):
            deleted.append(d)

    return added, deleted


def _externals(ctx):
    ext = svnexternals.externalsfile()
    if '.hgsvnexternals' in ctx:
        ext.read(ctx['.hgsvnexternals'].data())
    return ext


def commit_from_rev(ui, repo, rev_ctx, hg_editor, svn_url, base_revision,
                    username, password):
    """Build and send a commit from Mercurial to Subversion.
    """
    file_data = {}
    svn = svnwrap.SubversionRepo(svn_url, username, password)
    parent = rev_ctx.parents()[0]
    parent_branch = rev_ctx.parents()[0].branch()
    branch_path = 'trunk'

    if parent_branch and parent_branch != 'default':
        branch_path = 'branches/%s' % parent_branch

    extchanges = list(svnexternals.diff(_externals(parent),
                                        _externals(rev_ctx)))
    addeddirs, deleteddirs = _getdirchanges(svn, branch_path, parent, rev_ctx,
                                            rev_ctx.files(), extchanges)
    deleteddirs = set(deleteddirs)

    props = {}
    copies = {}
    for file in rev_ctx.files():
        if file == '.hgsvnexternals':
            continue
        new_data = base_data = ''
        action = ''
        if file in rev_ctx:
            fctx = rev_ctx.filectx(file)
            new_data = fctx.data()

            if 'x' in fctx.flags():
                props.setdefault(file, {})['svn:executable'] = '*'
            if 'l' in fctx.flags():
                props.setdefault(file, {})['svn:special'] = '*'

            if file not in parent:
                renamed = fctx.renamed()
                if renamed:
                    # TODO current model (and perhaps svn model) does not support
                    # this kind of renames: a -> b, b -> c
                    copies[file] = renamed[0]
                    base_data = parent[renamed[0]].data()

                action = 'add'
                dirname = '/'.join(file.split('/')[:-1] + [''])
            else:
                base_data = parent.filectx(file).data()
                if ('x' in parent.filectx(file).flags()
                    and 'x' not in rev_ctx.filectx(file).flags()):
                    props.setdefault(file, {})['svn:executable'] = None
                if ('l' in parent.filectx(file).flags()
                    and 'l' not in rev_ctx.filectx(file).flags()):
                    props.setdefault(file, {})['svn:special'] = None
                action = 'modify'
        else:
            pos = file.rfind('/')
            if pos >= 0:
                if file[:pos] in deleteddirs:
                    # This file will be removed when its directory is removed
                    continue
            action = 'delete'
        file_data[file] = base_data, new_data, action

    def svnpath(p):
        return '%s/%s' % (branch_path, p)

    changeddirs = []
    for d, v1, v2 in extchanges:
        props.setdefault(svnpath(d), {})['svn:externals'] = v2
        if d not in deleteddirs and d not in addeddirs:
            changeddirs.append(svnpath(d))

    # Now we are done with files, we can prune deleted directories
    # against themselves: ignore a/b if a/ is already removed
    deleteddirs2 = list(deleteddirs)
    deleteddirs2.sort(reverse=True)
    for d in deleteddirs2:
        pos = d.rfind('/')
        if pos >= 0 and d[:pos] in deleteddirs:
            deleteddirs.remove(d[:pos])

    newcopies = {}
    for source, dest in copies.iteritems():
        newcopies[svnpath(source)] = (svnpath(dest), base_revision)

    new_target_files = [svnpath(f) for f in file_data]
    for tf, ntf in zip(file_data, new_target_files):
        if tf in file_data:
            file_data[ntf] = file_data[tf]
            if tf in props:
                props[ntf] = props[tf]
                del props[tf]
            if hgutil.binary(file_data[ntf][1]):
                props.setdefault(ntf, {}).update(props.get(ntf, {}))
                props.setdefault(ntf, {})['svn:mime-type'] = 'application/octet-stream'
            del file_data[tf]

    addeddirs = [svnpath(d) for d in addeddirs]
    deleteddirs = [svnpath(d) for d in deleteddirs]
    new_target_files += addeddirs + deleteddirs + changeddirs
    if not new_target_files:
        raise NoFilesException()
    try:
        svn.commit(new_target_files, rev_ctx.description(), file_data,
                   base_revision, set(addeddirs), set(deleteddirs),
                   props, newcopies)
    except core.SubversionException, e:
        if hasattr(e, 'apr_err') and (e.apr_err == core.SVN_ERR_FS_TXN_OUT_OF_DATE
                                      or e.apr_err == core.SVN_ERR_FS_CONFLICT):
            raise hgutil.Abort('Base text was out of date, maybe rebase?')
        else:
            raise

    return True

def islocalrepo(url):
    if not url.startswith('file:///'):
        return False
    if '#' in url.split('/')[-1]: # strip off #anchor
        url = url[:url.rfind('#')]
    path = url[len('file://'):]
    path = urllib.url2pathname(path).replace(os.sep, '/')
    while '/' in path:
        if reduce(lambda x,y: x and y,
                  map(lambda p: os.path.exists(os.path.join(path, p)),
                      ('hooks', 'format', 'db', ))):
            return True
        path = path.rsplit('/', 1)[0]
    return False

def issvnurl(url):
    return url.startswith('svn') or islocalrepo(url)
