
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
