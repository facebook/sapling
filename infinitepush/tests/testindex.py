from __future__ import absolute_import
import os
from mercurial import hg, ui
from .util import getrandomid, getfileindexandrepo
from .. import indexapi

# tmpdir is some py.test magic!
def testfileindexapiinit(tmpdir):
    """Check that we can create a fileindexapi"""
    os.chdir(tmpdir.dirname)
    os.system("hg init")
    repo = hg.repository(ui.ui(), tmpdir.dirname)
    indexapi.fileindexapi(repo)

def testaddingretrievingbundle(tmpdir):
    fileindex, repo = getfileindexandrepo(tmpdir)
    bundleid = getrandomid()
    nodes = [getrandomid() for u in range(30)]
    fileindex.addbundle(bundleid, nodes)
    r = fileindex.getbundle(nodes[0])
    assert(r == bundleid)

def testaddingretrievingbookmark(tmpdir):
    fileindex, repo = getfileindexandrepo(tmpdir)
    bookmark = getrandomid()
    node = getrandomid()
    fileindex.addbookmark(bookmark, node)
    n = fileindex.getnode(bookmark)
    assert(n == node)

def testretrievingnonexistingbookmark(tmpdir):
    fileindex, repo = getfileindexandrepo(tmpdir)
    n = fileindex.getnode(getrandomid())
    assert n is None

def testretrievingnonexistingbundle(tmpdir):
    fileindex, repo = getfileindexandrepo(tmpdir)
    r = fileindex.getbundle(getrandomid())
    assert r is None
