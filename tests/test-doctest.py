# this is hack to make sure no escape characters are inserted into the output
import os, sys
if 'TERM' in os.environ:
    del os.environ['TERM']
import doctest

def testmod(name, optionflags=0, testtarget=None):
    __import__(name)
    mod = sys.modules[name]
    if testtarget is not None:
        mod = getattr(mod, testtarget)
    doctest.testmod(mod, optionflags=optionflags)

testmod('mercurial.changelog')
testmod('mercurial.dagparser', optionflags=doctest.NORMALIZE_WHITESPACE)
testmod('mercurial.dispatch')
testmod('mercurial.encoding')
testmod('mercurial.hgweb.hgwebdir_mod')
testmod('mercurial.match')
testmod('mercurial.minirst')
testmod('mercurial.revset')
testmod('mercurial.store')
testmod('mercurial.templatefilters')
testmod('mercurial.ui')
testmod('mercurial.url')
testmod('mercurial.util')
testmod('mercurial.util', testtarget='platform')
testmod('hgext.convert.cvsps')
