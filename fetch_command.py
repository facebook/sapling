import cStringIO
import re
import operator
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


def print_your_svn_is_old_message(ui):
    ui.status("In light of that, I'll fall back and do diffs, but it won't do "
              "as good a job. You should really upgrade your server.")


@util.register_subcommand('pull')
def fetch_revisions(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
                    tag_locations='tags',
                    **opts):
    """Pull new revisions from Subversion.
    """
    skipto_rev=int(skipto_rev)
    have_replay = not stupid
    if have_replay and not callable(delta.svn_txdelta_apply(None, None,
                                                            None)[0]):
        ui.status('You are using old Subversion SWIG bindings. Replay will not'
                  ' work until you upgrade to 1.5.0 or newer. Falling back to'
                  ' a slower method that may be buggier. Please upgrade, or'
                  ' contribute a patch to use the ctypes bindings instead'
                  ' of SWIG.')
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
                        except svnwrap.SubversionRepoCanNotReplay, e:
                            ui.status('%s\n' % e.message)
                            print_your_svn_is_old_message(ui)
                            have_replay = False
                            stupid_svn_server_pull_rev(ui, svn, hg_editor, r)
                    else:
                        stupid_svn_server_pull_rev(ui, svn, hg_editor, r)
                    converted = True
                    open(hg_editor.last_revision_handled_file,
                         'w').write(str(r.revnum))
                except core.SubversionException, e:
                    if hasattr(e, 'message') and (
                        'Server sent unexpected return value (502 Bad Gateway)'
                        ' in response to PROPFIND') in e.message:
                        tries += 1
                        ui.status('Got a 502, retrying (%s)\n' % tries)
                    else:
                        raise


def replay_convert_rev(hg_editor, svn, r):
    hg_editor.set_current_rev(r)
    svn.get_replay(r.revnum, hg_editor)
    if hg_editor.missing_plaintexts:
        files_to_grab = set()
        dirs_to_list = []
        props = {}
        for p in hg_editor.missing_plaintexts:
            p2 = p
            if svn.subdir:
                p2 = p2[len(svn.subdir)-1:]
            # this *sometimes* raises on me, and I have
            # no idea why. TODO(augie) figure out the why.
            try:
                pl = svn.proplist(p2, r.revnum, recurse=True)
            except core.SubversionException, e:
                pass
            props.update(pl)
            if p[-1] == '/':
                dirs_to_list.append(p)
            else:
                files_to_grab.add(p)
        while dirs_to_list:
            p = dirs_to_list.pop(0)
            l = svn.list_dir(p[:-1], r.revnum)
            for f in l:

                if l[f].kind == core.svn_node_dir:
                    dirs_to_list.append(p+f+'/')
                elif l[f].kind == core.svn_node_file:
                    files_to_grab.add(p+f)
        for p in files_to_grab:
            p2 = p
            if svn.subdir:
                p2 = p2[len(svn.subdir)-1:]
            hg_editor.current_files[p] = svn.get_file(p2, r.revnum)
            hg_editor.current_files_exec[p] = False
            if p in props:
                if 'svn:executable' in props[p]:
                    hg_editor.current_files_exec[p] = True
                if 'svn:special' in props[p]:
                    hg_editor.current_files_symlink[p] = True
        hg_editor.missing_plaintexts = set()
    hg_editor.commit_current_delta()


binary_file_re = re.compile(r'''Index: ([^\n]*)
=*
Cannot display: file marked as a binary type.''')

property_exec_set_re = re.compile(r'''Property changes on: ([^\n]*)
_*
Added: svn:executable
   \+ \*
''')

property_exec_removed_re = re.compile(r'''Property changes on: ([^\n]*)
_*
Deleted: svn:executable
   - \*
''')

empty_file_patch_wont_make_re = re.compile(r'''Index: ([^\n]*)\n=*\n(?=Index:)''')

any_file_re = re.compile(r'''^Index: ([^\n]*)\n=*\n''', re.MULTILINE)

property_special_set_re = re.compile(r'''Property changes on: ([^\n]*)
_*
Added: svn:special
   \+ \*
''')

property_special_removed_re = re.compile(r'''Property changes on: ([^\n]*)
_*
Added: svn:special
   \- \*
''')

def make_diff_path(b):
    if b == None:
        return 'trunk'
    return 'branches/' + b


def stupid_svn_server_pull_rev(ui, svn, hg_editor, r):
    used_diff = True
    delete_all_files = False
    # this server fails at replay
    branches = hg_editor.branches_in_paths(r.paths)
    temp_location = os.path.join(hg_editor.path, '.hg', 'svn', 'temp')
    if not os.path.exists(temp_location):
        os.makedirs(temp_location)
    for b in branches:
        our_tempdir = tempfile.mkdtemp('svn_fetch_temp', dir=temp_location)
        diff_path = make_diff_path(b)
        parent_rev, br_p = hg_editor.get_parent_svn_branch_and_rev(r.revnum, b)
        parent_ha = hg_editor.get_parent_revision(r.revnum, b)
        files_touched = set()
        link_files = {}
        exec_files = {}
        try:
            if br_p == b:
                d = svn.get_unified_diff(diff_path, r.revnum, deleted=False,
                                         # letting patch handle binaries sounded
                                         # cool, but it breaks patch in sad ways
                                         ignore_type=False)
            else:
                d = svn.get_unified_diff(diff_path, r.revnum,
                                         other_path=make_diff_path(br_p),
                                         other_rev=parent_rev,
                                         deleted=True, ignore_type=True)
                if d:
                    ui.status('Branch creation with mods, pulling full rev.\n')
                    raise BadPatchApply()
            for m in binary_file_re.findall(d):
                # we have to pull each binary file by hand as a fulltext,
                # which sucks but we've got no choice
                file_path = os.path.join(our_tempdir, m)
                files_touched.add(m)
                try:
                    try:
                        os.makedirs(os.path.dirname(file_path))
                    except OSError, e:
                        pass
                    f = open(file_path, 'w')
                    f.write(svn.get_file(diff_path+'/'+m, r.revnum))
                    f.close()
                except core.SubversionException, e:
                    if (e.message.endswith("' path not found")
                        or e.message.startswith("File not found: revision")):
                        pass
                    else:
                        raise
            d2 = empty_file_patch_wont_make_re.sub('', d)
            d2 = property_exec_set_re.sub('', d2)
            d2 = property_exec_removed_re.sub('', d2)
            old_cwd = os.getcwd()
            os.chdir(our_tempdir)
            for f in any_file_re.findall(d):
                files_touched.add(f)
                # this check is here because modified binary files will get
                # created before here.
                if os.path.exists(f):
                    continue
                dn = os.path.dirname(f)
                if dn and not os.path.exists(dn):
                    os.makedirs(dn)
                if f in hg_editor.repo[parent_ha].manifest():
                    data = hg_editor.repo[parent_ha].filectx(f).data()
                    fi = open(f, 'w')
                    fi.write(data)
                    fi.close()
                else:
                    open(f, 'w').close()
                if f.startswith(our_tempdir):
                    f = f[len(our_tempdir)+1:]
            os.chdir(old_cwd)
            if d2.strip() and len(re.findall('\n[-+]', d2.strip())) > 0:
                old_cwd = os.getcwd()
                os.chdir(our_tempdir)
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
                    raise BadPatchApply()
                for x in changed.iterkeys():
                    ui.status('M  %s\n' % x)
                    files_touched.add(x)
                os.chdir(old_cwd)
                # if this patch didn't apply right, fall back to exporting the
                # entire rev.
                if patch_st == -1:
                    parent_ctx = hg_editor.repo[parent_ha]
                    parent_manifest = parent_ctx.manifest()
                    for fn in files_touched:
                        if (fn in parent_manifest and
                            'l' in parent_ctx.filectx(fn).flags()):
                            # I think this might be an underlying bug in svn -
                            # I get diffs of deleted symlinks even though I
                            # specifically said no deletes above.
                            ui.status('Pulling whole rev because of a deleted'
                                      'symlink')
                            raise BadPatchApply()
                    assert False, ('This should only happen on case-insensitive'
                                   ' volumes.')
                elif patch_st == 1:
                    # When converting Django, I saw fuzz on .po files that was
                    # causing revisions to end up failing verification. If that
                    # can be fixed, maybe this won't ever be reached.
                    ui.status('There was some fuzz, not using diff after all.')
                    raise BadPatchApply()
            else:
                ui.status('Not using patch for %s, diff had no hunks.\n' %
                          r.revnum)

            # we create the files if they don't exist here because we know
            # that we'll never have diff info for a deleted file, so if the
            # property is set, we should force the file to exist no matter what.
            for m in property_exec_removed_re.findall(d):
                f = os.path.join(our_tempdir, m)
                if not os.path.exists(f):
                    d = os.path.dirname(f)
                    if not os.path.exists(d):
                        os.makedirs(d)
                    if not m in hg_editor.repo[parent_ha].manifest():
                        open(f, 'w').close()
                    else:
                        data = hg_editor.repo[parent_ha].filectx(m).data()
                        fp = open(f, 'w')
                        fp.write(data)
                        fp.close()
                exec_files[m] = False
                files_touched.add(m)
            for m in property_exec_set_re.findall(d):
                f = os.path.join(our_tempdir, m)
                if not os.path.exists(f):
                    d = os.path.dirname(f)
                    if not os.path.exists(d):
                        os.makedirs(d)
                    if m not in hg_editor.repo[parent_ha].manifest():
                        open(f, 'w').close()
                    else:
                        data = hg_editor.repo[parent_ha].filectx(m).data()
                        fp = open(f, 'w')
                        fp.write(data)
                        fp.close()
                exec_files[m] = True
                files_touched.add(m)
            for m in property_special_set_re.findall(d):
                # TODO(augie) when a symlink is removed, patching will fail.
                # We're seeing that above - there's gotta be a better
                # workaround than just bailing like that.
                path = os.path.join(our_tempdir, m)
                assert os.path.exists(path)
                link_path = open(path).read()
                link_path = link_path[len('link '):]
                os.remove(path)
                link_files[m] = link_path
                files_touched.add(m)
        except core.SubversionException, e:
            if (e.apr_err == 160013 or (hasattr(e, 'message') and
                  'was not found in the repository at revision ' in e.message)):
                # Either this revision or the previous one does not exist.
                try:
                    ui.status("fetching entire rev previous rev does not exist.\n")
                    used_diff = False
                    svn.fetch_all_files_to_dir(diff_path, r.revnum, our_tempdir)
                except core.SubversionException, e:
                    if e.apr_err == 170000 or (e.message.startswith("URL '")
                         and e.message.endswith("' doesn't exist")):
                        delete_all_files = True
                    else:
                        raise

        except BadPatchApply, e:
            # previous rev didn't exist, so this is most likely the first
            # revision. We'll have to pull all files by hand.
            try:
                ui.status("fetching entire rev because raised.\n")
                used_diff = False
                shutil.rmtree(our_tempdir)
                os.makedirs(our_tempdir)
                svn.fetch_all_files_to_dir(diff_path, r.revnum, our_tempdir)
            except core.SubversionException, e:
                if e.apr_err == 170000 or (e.message.startswith("URL '")
                     and e.message.endswith("' doesn't exist")):
                    delete_all_files = True
                else:
                    raise
        for p in r.paths:
            if p.startswith(diff_path) and r.paths[p].action == 'D':
                p2 =  p[len(diff_path)+1:]
                files_touched.add(p2)
                p3 = os.path.join(our_tempdir, p2)
                if os.path.exists(p3) and not os.path.isdir(p3):
                    os.unlink(p3)
                if p2 and p2[0] == '/':
                    p2 = p2[1:]
                # If this isn't in the parent ctx, it must've been a dir
                if not p2 in hg_editor.repo[parent_ha]:
                    d_files = [f for f in hg_editor.repo[parent_ha].manifest().iterkeys()
                               if f.startswith(p2 + '/')]
                    for d in d_files:
                        files_touched.add(d)
        if delete_all_files:
            for p in hg_editor.repo[parent_ha].manifest().iterkeys():
                files_touched.add(p)
        if not used_diff:
            for p in reduce(operator.add, [[os.path.join(x[0], y) for y in x[2]]
                                           for x in
                                           list(os.walk(our_tempdir))]):
                p_real = p[len(our_tempdir)+1:]
                if os.path.islink(p):
                    link_files[p_real] = os.readlink(p)
                exec_files[p_real] = (os.lstat(p).st_mode & 0100 != 0)
                files_touched.add(p_real)
            for p in hg_editor.repo[parent_ha].manifest().iterkeys():
                # TODO this might not be a required step.
                files_touched.add(p)
        date = r.date.replace('T', ' ').replace('Z', '').split('.')[0]
        date += ' -0000'
        def filectxfn(repo, memctx, path):
            disk_path = os.path.join(our_tempdir, path)
            if path in link_files:
                return context.memfilectx(path=path, data=link_files[path],
                                          islink=True, isexec=False,
                                          copied=False)
            fp = open(disk_path)
            exe = exec_files.get(path, None)
            if exe is None and path in hg_editor.repo[parent_ha]:
                exe = 'x' in hg_editor.repo[parent_ha].filectx(path).flags()
            return context.memfilectx(path=path, data=fp.read(), islink=False,
                                      isexec=exe, copied=False)
        extra = {}
        if b:
            extra['branch'] = b
        if parent_ha != node.nullid or files_touched:
            # TODO(augie) remove this debug code? Or maybe it's sane to have it.
            for f in files_touched:
                if f:
                    assert f[0] != '/'
            current_ctx = context.memctx(hg_editor.repo,
                                         [parent_ha, revlog.nullid],
                                         r.message or '...',
                                         files_touched,
                                         filectxfn,
                                         '%s%s' % (r.author,
                                                   hg_editor.author_host),
                                         date,
                                         extra)
            ha = hg_editor.repo.commitctx(current_ctx)
            hg_editor.revmap[r.revnum, b] = ha
            hg_editor._save_metadata()
            ui.status('committed as %s on branch %s\n' %
                      (node.hex(ha),  b or 'default'))
        shutil.rmtree(our_tempdir)


class BadPatchApply(Exception):
    pass
