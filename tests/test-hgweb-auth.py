from mercurial import demandimport; demandimport.enable()
from mercurial import ui
from mercurial import url
from mercurial.error import Abort

class myui(ui.ui):
    def interactive(self):
        return False

origui = myui()

def writeauth(items):
    ui = origui.copy()
    for name, value in items.iteritems():
        ui.setconfig('auth', name, value)
    return ui

def dumpdict(dict):
    return '{' + ', '.join(['%s: %s' % (k, dict[k])
                            for k in sorted(dict.iterkeys())]) + '}'

def test(auth):
    print 'CFG:', dumpdict(auth)
    prefixes = set()
    for k in auth:
        prefixes.add(k.split('.', 1)[0])
    for p in prefixes:
        auth.update({p + '.username': p, p + '.password': p})

    ui = writeauth(auth)

    def _test(uri):
        print 'URI:', uri
        try:
            pm = url.passwordmgr(ui)
            print '    ', pm.find_user_password('test', uri)
        except Abort, e:
            print 'abort'

    _test('http://example.org/foo')
    _test('http://example.org/foo/bar')
    _test('http://example.org/bar')
    _test('https://example.org/foo')
    _test('https://example.org/foo/bar')
    _test('https://example.org/bar')


print '\n*** Test in-uri schemes\n'
test({'x.prefix': 'http://example.org'})
test({'x.prefix': 'https://example.org'})
test({'x.prefix': 'http://example.org', 'x.schemes': 'https'})
test({'x.prefix': 'https://example.org', 'x.schemes': 'http'})

print '\n*** Test separately configured schemes\n'
test({'x.prefix': 'example.org', 'x.schemes': 'http'})
test({'x.prefix': 'example.org', 'x.schemes': 'https'})
test({'x.prefix': 'example.org', 'x.schemes': 'http https'})

print '\n*** Test prefix matching\n'
test({'x.prefix': 'http://example.org/foo',
      'y.prefix': 'http://example.org/bar'})
test({'x.prefix': 'http://example.org/foo',
      'y.prefix': 'http://example.org/foo/bar'})
test({'x.prefix': '*', 'y.prefix': 'https://example.org/bar'})
