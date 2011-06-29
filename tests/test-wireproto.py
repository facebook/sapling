from mercurial import wireproto

class proto(object):
    def __init__(self, args):
        self.args = args
    def getargs(self, spec):
        args = self.args
        args.setdefault('*', {})
        names = spec.split()
        return [args[n] for n in names]

class clientrepo(wireproto.wirerepository):
    def __init__(self, serverrepo):
        self.serverrepo = serverrepo
    def _call(self, cmd, **args):
        return wireproto.dispatch(self.serverrepo, proto(args), cmd)

    @wireproto.batchable
    def greet(self, name):
        f = wireproto.future()
        yield wireproto.todict(name=mangle(name)), f
        yield unmangle(f.value)

class serverrepo(object):
    def greet(self, name):
        return "Hello, " + name

def mangle(s):
    return ''.join(chr(ord(c) + 1) for c in s)
def unmangle(s):
    return ''.join(chr(ord(c) - 1) for c in s)

def greet(repo, proto, name):
    return mangle(repo.greet(unmangle(name)))

wireproto.commands['greet'] = (greet, 'name',)

srv = serverrepo()
clt = clientrepo(srv)

print clt.greet("Foobar")
b = clt.batch()
fs = [b.greet(s) for s in ["Fo, =;o", "Bar"]]
b.submit()
print [f.value for f in fs]
