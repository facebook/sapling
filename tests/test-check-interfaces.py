# Test that certain objects conform to well-defined interfaces.

from __future__ import absolute_import, print_function

from mercurial import (
    bundlerepo,
    httppeer,
    localrepo,
    sshpeer,
    statichttprepo,
    ui as uimod,
    unionrepo,
)

def checkobject(o):
    """Verify a constructed object conforms to interface rules.

    An object must have __abstractmethods__ defined.

    All "public" attributes of the object (attributes not prefixed with
    an underscore) must be in __abstractmethods__ or appear on a base class
    with __abstractmethods__.
    """
    name = o.__class__.__name__

    allowed = set()
    for cls in o.__class__.__mro__:
        if not getattr(cls, '__abstractmethods__', set()):
            continue

        allowed |= cls.__abstractmethods__
        allowed |= {a for a in dir(cls) if not a.startswith('_')}

    if not allowed:
        print('%s does not have abstract methods' % name)
        return

    public = {a for a in dir(o) if not a.startswith('_')}

    for attr in sorted(public - allowed):
        print('public attributes not in abstract interface: %s.%s' % (
            name, attr))

# Facilitates testing localpeer.
class dummyrepo(object):
    def __init__(self):
        self.ui = uimod.ui()
    def filtered(self, name):
        pass
    def _restrictcapabilities(self, caps):
        pass

# Facilitates testing sshpeer without requiring an SSH server.
class testingsshpeer(sshpeer.sshpeer):
    def _validaterepo(self, *args, **kwargs):
        pass

class badpeer(httppeer.httppeer):
    def __init__(self):
        super(badpeer, self).__init__(uimod.ui(), 'http://localhost')
        self.badattribute = True

    def badmethod(self):
        pass

def main():
    ui = uimod.ui()

    checkobject(badpeer())
    checkobject(httppeer.httppeer(ui, 'http://localhost'))
    checkobject(localrepo.localpeer(dummyrepo()))
    checkobject(testingsshpeer(ui, 'ssh://localhost/foo'))
    checkobject(bundlerepo.bundlepeer(dummyrepo()))
    checkobject(statichttprepo.statichttppeer(dummyrepo()))
    checkobject(unionrepo.unionpeer(dummyrepo()))

main()
