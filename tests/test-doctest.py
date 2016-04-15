# this is hack to make sure no escape characters are inserted into the output

from __future__ import absolute_import

import doctest
import os
import sys
if 'TERM' in os.environ:
    del os.environ['TERM']

def testmod(name, optionflags=0, testtarget=None):
    __import__(name)
    mod = sys.modules[name]
    if testtarget is not None:
        mod = getattr(mod, testtarget)
    doctest.testmod(mod, optionflags=optionflags)

testmod('mercurial.changegroup')
testmod('mercurial.changelog')
testmod('mercurial.dagparser', optionflags=doctest.NORMALIZE_WHITESPACE)
testmod('mercurial.dispatch')
testmod('mercurial.encoding')
testmod('mercurial.hg')
testmod('mercurial.hgweb.hgwebdir_mod')
testmod('mercurial.match')
testmod('mercurial.minirst')
testmod('mercurial.patch')
testmod('mercurial.pathutil')
testmod('mercurial.parser')
testmod('mercurial.revset')
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
