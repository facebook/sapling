import os
import popen2

from mercurial import ui
from mercurial import hg

import fetch_command

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')

def load_svndump_fixture(path, fixture_name):
    '''Loads an svnadmin dump into a fresh repo at path, which should not
    already exist.
    '''
    os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create', path,])
    proc = popen2.Popen4(['svnadmin', 'load', path,])
    inp = open(os.path.join(FIXTURES, fixture_name))
    proc.tochild.write(inp.read())
    proc.tochild.close()
    proc.wait()

def load_fixture_and_fetch(fixture_name, repo_path, wc_path, stupid=False):
    load_svndump_fixture(repo_path, fixture_name)
    fetch_command.fetch_revisions(ui.ui(),
                                  svn_url='file://%s' % repo_path,
                                  hg_repo_path=wc_path,
                                  stupid=stupid)
    repo = hg.repository(ui.ui(), wc_path)
    return repo
