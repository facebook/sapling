from __future__ import absolute_import
from .util import getrepo, getfilebundlestore, getrandomid
from mercurial import ui
from .. import store

# tmpdir is some py.test magic!
def teststoreinit(tmpdir):
    repo = getrepo(tmpdir)
    store.filebundlestore(ui.ui(), repo)

def testwriteandread(tmpdir):
    bundlestore = getfilebundlestore(tmpdir)
    _id = getrandomid()
    r = bundlestore.write(_id)
    data = bundlestore.read(r)
    assert _id == data

