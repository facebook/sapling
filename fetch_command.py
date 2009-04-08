import os

from mercurial import util as merc_util
from svn import core
from svn import delta

import hg_delta_editor
import svnwrap
import stupid as stupidmod
import util


def fetch_revisions(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
                    tag_locations='tags',
                    authors=None,
                    filemap=None,
                    **opts):
    """pull new revisions from Subversion
    """
    svn_url = util.normalize_url(svn_url)
    old_encoding = util.swap_out_encoding()
    skipto_rev=int(skipto_rev)
    have_replay = not stupid
    if have_replay and not callable(
        delta.svn_txdelta_apply(None, None, None)[0]): #pragma: no cover
        ui.status('You are using old Subversion SWIG bindings. Replay will not'
                  ' work until you upgrade to 1.5.0 or newer. Falling back to'
                  ' a slower method that may be buggier. Please upgrade, or'
                  ' contribute a patch to use the ctypes bindings instead'
                  ' of SWIG.\n')
        have_replay = False
    initializing_repo = False
    user = opts.get('username', merc_util.getuser())
    passwd = opts.get('password', '')
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    author_host = "@%s" % svn.uuid
    tag_locations = tag_locations.split(',')
    hg_editor = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                                 ui_=ui,
                                                 subdir=svn.subdir,
                                                 author_host=author_host,
                                                 tag_locations=tag_locations,
                                                 authors=authors,
                                                 filemap=filemap)
    if os.path.exists(hg_editor.uuid_file):
        uuid = open(hg_editor.uuid_file).read()
        assert uuid == svn.uuid
        start = hg_editor.last_known_revision()
    else:
        open(hg_editor.uuid_file, 'w').write(svn.uuid)
        open(hg_editor.svn_url_file, 'w').write(svn_url)
        initializing_repo = True
        start = skipto_rev

    if initializing_repo and start > 0:
        raise merc_util.Abort('Revision skipping at repository initialization '
                              'remains unimplemented.')

    # start converting revisions
    for r in svn.revisions(start=start):
        valid = True
        hg_editor.update_branch_tag_map_for_rev(r)
        for p in r.paths:
            if hg_editor._is_path_valid(p):
                valid = True
                break
        if valid:
            # got a 502? Try more than once!
            tries = 0
            converted = False
            while not converted:
                try:
                    util.describe_revision(ui, r)
                    if have_replay:
                        try:
                            replay_convert_rev(hg_editor, svn, r)
                        except svnwrap.SubversionRepoCanNotReplay, e: #pragma: no cover
                            ui.status('%s\n' % e.message)
                            stupidmod.print_your_svn_is_old_message(ui)
                            have_replay = False
                            stupidmod.svn_server_pull_rev(ui, svn, hg_editor, r)
                    else:
                        stupidmod.svn_server_pull_rev(ui, svn, hg_editor, r)
                    converted = True
                except core.SubversionException, e: #pragma: no cover
                    if (e.apr_err == core.SVN_ERR_RA_DAV_REQUEST_FAILED
                        and '502' in str(e)
                        and tries < 3):
                        tries += 1
                        ui.status('Got a 502, retrying (%s)\n' % tries)
                    else:
                        raise merc_util.Abort(*e.args)
    util.swap_out_encoding(old_encoding)

fetch_revisions = util.register_subcommand('pull')(fetch_revisions)


def cleanup_file_handles(svn, count):
    if count % 50 == 0:
        svn.init_ra_and_client()


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
            cleanup_file_handles(svn, i)
            i += 1
            data, mode = svn.get_file(p, r.revnum)
            hg_editor.set_file(p, data, 'x' in mode, 'l' in mode)
        hg_editor.missing_plaintexts = set()
        hg_editor.ui.note('\n')
    hg_editor.commit_current_delta()
