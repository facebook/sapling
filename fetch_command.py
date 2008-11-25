import cStringIO
import re
import os
import shutil
import tempfile

from mercurial import patch
from mercurial import node
from mercurial import context
from mercurial import revlog
from mercurial import util as merc_util
from svn import core
from svn import delta

import hg_delta_editor
import svnwrap
import util


def print_your_svn_is_old_message(ui): #pragma: no cover
    ui.status("In light of that, I'll fall back and do diffs, but it won't do "
              "as good a job. You should really upgrade your server.\n")


@util.register_subcommand('pull')
def fetch_revisions(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
                    tag_locations='tags',
                    **opts):
    """Pull new revisions from Subversion.
    """
    old_encoding = merc_util._encoding
    merc_util._encoding = 'UTF-8'
    skipto_rev=int(skipto_rev)
    have_replay = not stupid
    if have_replay and not callable(delta.svn_txdelta_apply(None, None,
                                                            None)[0]): #pragma: no cover
        ui.status('You are using old Subversion SWIG bindings. Replay will not'
                  ' work until you upgrade to 1.5.0 or newer. Falling back to'
                  ' a slower method that may be buggier. Please upgrade, or'
                  ' contribute a patch to use the ctypes bindings instead'
                  ' of SWIG.\n')
        have_replay = False
    initializing_repo = False
    svn = svnwrap.SubversionRepo(svn_url, username=merc_util.getuser())
    author_host = "@%s" % svn.uuid
    tag_locations = tag_locations.split(',')
    hg_editor = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                                 ui_=ui,
                                                 subdir=svn.subdir,
                                                 author_host=author_host,
                                                 tag_locations=tag_locations)
    if os.path.exists(hg_editor.uuid_file):
        uuid = open(hg_editor.uuid_file).read()
        assert uuid == svn.uuid
        start = int(open(hg_editor.last_revision_handled_file, 'r').read())
    else:
        open(hg_editor.uuid_file, 'w').write(svn.uuid)
        open(hg_editor.svn_url_file, 'w').write(svn_url)
        open(hg_editor.last_revision_handled_file, 'w').write(str(0))
        initializing_repo = True
        start = skipto_rev

    # start converting revisions
    for r in svn.revisions(start=start):
        valid = False
        hg_editor.update_branch_tag_map_for_rev(r)
        for p in r.paths:
            if hg_editor._is_path_valid(p):
                valid = True
                continue
        if initializing_repo and start > 0:
            assert False, 'This feature not ready yet.'
        if valid:
            # got a 502? Try more than once!
            tries = 0
            converted = False
            while not converted and tries < 3:
                try:
                    ui.status('converting %s\n' % r)
                    if have_replay:
                        try:
                            replay_convert_rev(hg_editor, svn, r)
                        except svnwrap.SubversionRepoCanNotReplay, e: #pragma: no cover
                            ui.status('%s\n' % e.message)
                            print_your_svn_is_old_message(ui)
                            have_replay = False
                            stupid_svn_server_pull_rev(ui, svn, hg_editor, r)
                    else:
                        stupid_svn_server_pull_rev(ui, svn, hg_editor, r)
                    converted = True
                    tmpfile = '%s_tmp' % hg_editor.last_revision_handled_file
                    fp = open(tmpfile, 'w')
                    fp.write(str(r.revnum))
                    fp.close()
                    merc_util.rename(tmpfile,
                                     hg_editor.last_revision_handled_file)
                except core.SubversionException, e: #pragma: no cover
                    if hasattr(e, 'message') and (
                        'Server sent unexpected return value (502 Bad Gateway)'
                        ' in response to PROPFIND') in e.message:
                        tries += 1
                        ui.status('Got a 502, retrying (%s)\n' % tries)
                    else:
                        raise
    merc_util._encoding = old_encoding


def cleanup_file_handles(svn, count):
    if count % 50 == 0:
        svn.init_ra_and_client()

def replay_convert_rev(hg_editor, svn, r):
    hg_editor.set_current_rev(r)
    svn.get_replay(r.revnum, hg_editor)
    i = 1
    if hg_editor.missing_plaintexts:
        hg_editor.ui.status('Fetching %s files that could not use replay.\n' %
                            len(hg_editor.missing_plaintexts))
        files_to_grab = set()
        rootpath = svn.subdir and svn.subdir[1:] or ''
        for p in hg_editor.missing_plaintexts:
            hg_editor.ui.status('.')
            hg_editor.ui.flush()
            if p[-1] == '/':
                dirpath = p[len(rootpath):]
                files_to_grab.update((dirpath + f for f,k in
                                      svn.list_files(dirpath, r.revnum) 
                                      if k == 'f'))
            else:
                files_to_grab.add(p[len(rootpath):])
        hg_editor.ui.status('\nFetching files...\n')
        for p in files_to_grab:
            hg_editor.ui.status('.')
            hg_editor.ui.flush()
            cleanup_file_handles(svn, i)
            i += 1
            data, mode = svn.get_file(p, r.revnum)
            hg_editor.set_file(p, data, 'x' in mode, 'l' in mode)
        hg_editor.missing_plaintexts = set()
        hg_editor.ui.status('\n')
    hg_editor.commit_current_delta()


binary_file_re = re.compile(r'''Index: ([^\n]*)
=*
Cannot display: file marked as a binary type.''')

property_exec_set_re = re.compile(r'''Property changes on: ([^\n]*)
_*
(?:Added|Name): svn:executable
   \+''')

property_exec_removed_re = re.compile(r'''Property changes on: ([^\n]*)
_*
(?:Deleted|Name): svn:executable
   -''')

empty_file_patch_wont_make_re = re.compile(r'''Index: ([^\n]*)\n=*\n(?=Index:)''')

any_file_re = re.compile(r'''^Index: ([^\n]*)\n=*\n''', re.MULTILINE)

property_special_set_re = re.compile(r'''Property changes on: ([^\n]*)
_*
(?:Added|Name): svn:special
   \+''')

property_special_removed_re = re.compile(r'''Property changes on: ([^\n]*)
_*
(?:Deleted|Name): svn:special
   \-''')

def stupid_diff_branchrev(ui, svn, hg_editor, branch, r, parentctx, tempdir):
    """Extract all 'branch' content at a given revision.

    Return a tuple (files, filectxfn) where 'files' is the list of all files
    in the branch at the given revision, and 'filectxfn' is a memctx compatible
    callable to retrieve individual file information. Raise BadPatchApply upon
    error.
    """
    def make_diff_path(b):
        if b == None:
            return 'trunk'
        return 'branches/' + b

    parent_rev, br_p = hg_editor.get_parent_svn_branch_and_rev(r.revnum, branch)
    diff_path = make_diff_path(branch)
    files_touched = set()
    try:
        if br_p == branch:
            # letting patch handle binaries sounded
            # cool, but it breaks patch in sad ways
            d = svn.get_unified_diff(diff_path, r.revnum, deleted=False,
                                     ignore_type=False)
        else:
            d = svn.get_unified_diff(diff_path, r.revnum,
                                     other_path=make_diff_path(br_p),
                                     other_rev=parent_rev,
                                     deleted=True, ignore_type=True)
            if d:
                raise BadPatchApply('branch creation with mods')
    except svnwrap.SubversionRepoCanNotDiff:
        raise BadPatchApply('subversion diffing code is not supported')
    except core.SubversionException, e:
        if (hasattr(e, 'apr_err') and e.apr_err != 160013):
            raise
        raise BadPatchApply('previous revision does not exist')
    opener = merc_util.opener(tempdir)
    for m in binary_file_re.findall(d):
        # we have to pull each binary file by hand as a fulltext,
        # which sucks but we've got no choice
        files_touched.add(m)
        try:
            f = opener(m, 'w')
            f.write(svn.get_file(diff_path+'/'+m, r.revnum)[0])
            f.close()
        except IOError:
            pass
    d2 = empty_file_patch_wont_make_re.sub('', d)
    d2 = property_exec_set_re.sub('', d2)
    d2 = property_exec_removed_re.sub('', d2)
    for f in any_file_re.findall(d):
        if f in files_touched:
            # this check is here because modified binary files will get
            # created before here.
            continue
        files_touched.add(f)
        data = ''
        if f in parentctx:
            data = parentctx[f].data()
        fp = opener(f, 'w')
        fp.write(data)
        fp.close()
    if d2.strip() and len(re.findall('\n[-+]', d2.strip())) > 0:
        old_cwd = os.getcwd()
        os.chdir(tempdir)
        changed = {}
        try:
            patch_st = patch.applydiff(ui, cStringIO.StringIO(d2),
                                       changed, strip=0)
        except patch.PatchError:
            # TODO: this happens if the svn server has the wrong mime
            # type stored and doesn't know a file is binary. It would
            # be better to do one file at a time and only do a
            # full fetch on files that had problems.
            os.chdir(old_cwd)
            raise BadPatchApply('patching failed')
        for x in changed.iterkeys():
            ui.status('M  %s\n' % x)
            files_touched.add(x)
        os.chdir(old_cwd)
        # if this patch didn't apply right, fall back to exporting the
        # entire rev.
        if patch_st == -1:
            for fn in files_touched:
                if 'l' in parentctx.flags(fn):
                    # I think this might be an underlying bug in svn -
                    # I get diffs of deleted symlinks even though I
                    # specifically said no deletes above.
                    raise BadPatchApply('deleted symlinked prevent patching')
            assert False, ('This should only happen on case-insensitive'
                           ' volumes.')
        elif patch_st == 1:
            # When converting Django, I saw fuzz on .po files that was
            # causing revisions to end up failing verification. If that
            # can be fixed, maybe this won't ever be reached.
            raise BadPatchApply('patching succeeded with fuzz')
    else:
        ui.status('Not using patch for %s, diff had no hunks.\n' %
                  r.revnum)

    # we create the files if they don't exist here because we know
    # that we'll never have diff info for a deleted file, so if the
    # property is set, we should force the file to exist no matter what.
    exec_files = {}
    for m in property_exec_removed_re.findall(d):
        exec_files[m] = False
    for m in property_exec_set_re.findall(d):
        exec_files[m] = True
    for m in exec_files:
        files_touched.add(m)
        f = os.path.join(tempdir, m)
        if not os.path.exists(f):
            data = ''
            if  m in parentctx:
                data = parentctx[m].data()
            fp = opener(m, 'w')
            fp.write(data)
            fp.close()
    link_files = {}
    for m in property_special_set_re.findall(d):
        # TODO(augie) when a symlink is removed, patching will fail.
        # We're seeing that above - there's gotta be a better
        # workaround than just bailing like that.
        path = os.path.join(tempdir, m)
        assert os.path.exists(path)
        link_path = open(path).read()
        link_path = link_path[len('link '):]
        os.remove(path)
        link_files[m] = link_path
        files_touched.add(m)

    deleted_files = set()
    for p in r.paths:
        if p.startswith(diff_path) and r.paths[p].action == 'D':
            p2 = p[len(diff_path)+1:].strip('/')
            if p2 in parentctx:
                deleted_files.add(p2)
                continue
            # If this isn't in the parent ctx, it must've been a dir
            deleted_files.update([f for f in parentctx if f.startswith(p2 + '/')])
    files_touched.update(deleted_files)

    copies = getcopies(svn, hg_editor, branch, diff_path, r, files_touched,
                       parentctx)

    def filectxfn(repo, memctx, path):
        if path in deleted_files:
            raise IOError()
        if path in link_files:
            return context.memfilectx(path=path, data=link_files[path],
                                      islink=True, isexec=False,
                                      copied=False)
        data = opener(path).read()
        exe = exec_files.get(path, 'x' in parentctx.flags(path))
        copied = copies.get(path)
        return context.memfilectx(path=path, data=data, islink=False,
                                  isexec=exe, copied=copied)

    return list(files_touched), filectxfn

def makecopyfinder(r, branchpath, rootdir):
    """Return a function detecting copies.

    Returned copyfinder(path) returns None if no copy information can
    be found or ((source, sourcerev), sourcepath) where "sourcepath" is the
    copy source path, "sourcerev" the source svn revision and "source" is the
    copy record path causing the copy to occur. If a single file was copied
    "sourcepath" and "source" are the same, while file copies dectected from
    directory copies return the copied source directory in "source".
    """
    # filter copy information for current branch
    branchpath = branchpath + '/'
    fullbranchpath = rootdir + branchpath
    copies = []
    for path, e in r.paths.iteritems():
        if not e.copyfrom_path:
            continue
        if not path.startswith(branchpath):
            continue
        if not e.copyfrom_path.startswith(fullbranchpath):
            # ignore cross branch copies
            continue
        dest = path[len(branchpath):]
        source = e.copyfrom_path[len(fullbranchpath):]
        copies.append((dest, (source, e.copyfrom_rev)))

    copies.sort()
    copies.reverse()
    exactcopies = dict(copies)

    def finder(path):
        if path in exactcopies:
            return exactcopies[path], exactcopies[path][0]
        # look for parent directory copy, longest first
        for dest, (source, sourcerev) in copies:
            dest = dest + '/'
            if not path.startswith(dest):
                continue
            sourcepath = source + '/' + path[len(dest):]
            return (source, sourcerev), sourcepath
        return None

    return finder

def getcopies(svn, hg_editor, branch, branchpath, r, files, parentctx):
    """Return a mapping {dest: source} for every file copied into r.
    """
    if parentctx.node() == revlog.nullid:
        return {}

    # Extract svn copy information, group them by copy source.
    # The idea is to duplicate the replay behaviour where copies are
    # evaluated per copy event (one event for all files in a directory copy,
    # one event for single file copy). We assume that copy events match
    # copy sources in revision info.
    svncopies = {}
    finder = makecopyfinder(r, branchpath, svn.subdir)
    for f in files:
        copy = finder(f)
        if copy:
            svncopies.setdefault(copy[0], []).append((f, copy[1]))
    if not svncopies:
        return {}

    # cache changeset contexts and map them to source svn revisions
    ctxs = {}
    def getctx(svnrev):
        if svnrev in ctxs:
            return ctxs[svnrev]
        changeid = hg_editor.get_parent_revision(svnrev + 1, branch)
        ctx = None
        if changeid != revlog.nullid:
            ctx = hg_editor.repo.changectx(changeid)
        ctxs[svnrev] = ctx
        return ctx

    # check svn copies really make sense in mercurial
    hgcopies = {}
    for (sourcepath, rev), copies in svncopies.iteritems():
        sourcectx = getctx(rev)
        if sourcectx is None:
            continue
        sources = [s[1] for s in copies]
        if not hg_editor.aresamefiles(sourcectx, parentctx, sources):
            continue
        hgcopies.update(copies)
    return hgcopies

def stupid_fetch_branchrev(svn, hg_editor, branch, branchpath, r, parentctx):
    """Extract all 'branch' content at a given revision.

    Return a tuple (files, filectxfn) where 'files' is the list of all files
    in the branch at the given revision, and 'filectxfn' is a memctx compatible
    callable to retrieve individual file information.
    """
    kind = svn.checkpath(branchpath, r.revnum)
    if kind is None:
        # Branch does not exist at this revision. Get parent revision and
        # remove everything.
        files = parentctx.manifest().keys()
        def filectxfn_rm(repo, memctx, path):
            raise IOError()
        return files, filectxfn_rm

    files = []
    if parentctx.node() == revlog.nullid:
        # Initial revision, fetch all files
        for path, kind in svn.list_files(branchpath, r.revnum):
            if kind == 'f':
                files.append(path)
    else:
        branchprefix = branchpath + '/'
        for path, e in r.paths.iteritems():
            if not path.startswith(branchprefix):
                continue
            kind = svn.checkpath(path, r.revnum)
            path = path[len(branchprefix):]
            if kind == 'f':
                files.append(path)
            elif kind == 'd':
                if e.action == 'M':
                    # Ignore property changes for now
                    continue
                dirpath = branchprefix + path
                for child, k in svn.list_files(dirpath, r.revnum):
                    if k == 'f':
                        files.append(path + '/' + child)
            else:
                if path in parentctx:
                    files.append(path)
                    continue
                # Assume it's a deleted directory
                path = path + '/'
                deleted = [f for f in parentctx if f.startswith(path)]
                files += deleted

    copies = getcopies(svn, hg_editor, branch, branchpath, r, files, parentctx)

    def filectxfn(repo, memctx, path):
        data, mode = svn.get_file(branchpath + '/' + path, r.revnum)
        isexec = 'x' in mode
        islink = 'l' in mode
        copied = copies.get(path)
        return context.memfilectx(path=path, data=data, islink=islink,
                                  isexec=isexec, copied=copied)

    return files, filectxfn

def stupid_svn_server_pull_rev(ui, svn, hg_editor, r):
    # this server fails at replay
    branches = hg_editor.branches_in_paths(r.paths)
    temp_location = os.path.join(hg_editor.path, '.hg', 'svn', 'temp')
    if not os.path.exists(temp_location):
        os.makedirs(temp_location)
    for b in branches:
        our_tempdir = tempfile.mkdtemp('svn_fetch_temp', dir=temp_location)
        parentctx = hg_editor.repo[hg_editor.get_parent_revision(r.revnum, b)]
        try:
            files_touched, filectxfn = stupid_diff_branchrev(
                ui, svn, hg_editor, b, r, parentctx, our_tempdir)
        except BadPatchApply, e:
            # Either this revision or the previous one does not exist.
            ui.status("fetching entire rev: %s.\n" % e.message)
            files_touched, filectxfn = stupid_fetch_branchrev(
                svn, hg_editor, b, branches[b], r, parentctx)

        date = r.date.replace('T', ' ').replace('Z', '').split('.')[0]
        date += ' -0000'
        extra = {}
        if b:
            extra['branch'] = b
        if '' in files_touched:
            files_touched.remove('')
        if parentctx.node() != node.nullid or files_touched:
            # TODO(augie) remove this debug code? Or maybe it's sane to have it.
            for f in files_touched:
                if f:
                    assert f[0] != '/'
            current_ctx = context.memctx(hg_editor.repo,
                                         [parentctx.node(), revlog.nullid],
                                         r.message or '...',
                                         files_touched,
                                         filectxfn,
                                         '%s%s' % (r.author,
                                                   hg_editor.author_host),
                                         date,
                                         extra)
            ha = hg_editor.repo.commitctx(current_ctx)
            hg_editor.add_to_revmap(r.revnum, b, ha)
            hg_editor._save_metadata()
            ui.status('committed as %s on branch %s\n' %
                      (node.hex(ha),  b or 'default'))
        if our_tempdir is not None:
            shutil.rmtree(our_tempdir)


class BadPatchApply(Exception):
    pass
