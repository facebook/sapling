# this is hack to make sure no escape characters are inserted into the output

from __future__ import absolute_import

import doctest
import os
import sys

ispy3 = (sys.version_info[0] >= 3)

if 'TERM' in os.environ:
    del os.environ['TERM']

# TODO: migrate doctests to py3 and enable them on both versions
def testmod(name, optionflags=0, testtarget=None, py2=True, py3=False):
    if not (not ispy3 and py2 or ispy3 and py3):
        return
    __import__(name)
    mod = sys.modules[name]
    if testtarget is not None:
        mod = getattr(mod, testtarget)
    doctest.testmod(mod, optionflags=optionflags)

testmod('mercurial.changegroup')
testmod('mercurial.changelog')
testmod('mercurial.color')
testmod('mercurial.config')
testmod('mercurial.context')
testmod('mercurial.dagparser', optionflags=doctest.NORMALIZE_WHITESPACE)
testmod('mercurial.dispatch')
testmod('mercurial.encoding')
testmod('mercurial.formatter')
testmod('mercurial.hg')
testmod('mercurial.hgweb.hgwebdir_mod')
testmod('mercurial.match')
testmod('mercurial.mdiff')
testmod('mercurial.minirst')
testmod('mercurial.patch')
testmod('mercurial.pathutil')
testmod('mercurial.parser')
testmod('mercurial.pycompat', py3=True)
testmod('mercurial.revsetlang')
testmod('mercurial.smartset')
testmod('mercurial.store')
testmod('mercurial.subrepo')
testmod('mercurial.templatefilters')
testmod('mercurial.templater')
testmod('mercurial.ui')
testmod('mercurial.url')
testmod('mercurial.util')
testmod('mercurial.util', testtarget='platform')
testmod('hgext.convert.convcmd')
testmod('hgext.convert.cvsps')
testmod('hgext.convert.filemap')
testmod('hgext.convert.p4')
testmod('hgext.convert.subversion')
testmod('hgext.mq')
