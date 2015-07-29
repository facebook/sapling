"""b2xcompat - temporary support for HG2Y format

The HG2Y format was used to experiment the "bundle2" feature. When the format
was frozen, a bunch of name were change to drop there experimental "flag",
However, the actual format and part handler did not changed. This small
extension makes it possible to discuss between the experimental version and the
final version. This will give a small windows to upgrade all client and server.
"""
import urllib
from mercurial import bundle2
from mercurial import exchange
from mercurial import extensions



def uisetup(ui):
    parthandlermapping = bundle2.parthandlermapping
    # double register all handlers
    for key, func in parthandlermapping.items():
        if not key.startswith('b2x:'):
            parthandlermapping['b2x:' + key] = func
    # if no unbundler for HG2Y, register one
    bundle2.formatmap.setdefault('2Y', bundle2.unbundle20)
    # see each wrapper for details
    extensions.wrapfunction(bundle2, 'getrepocaps', wrapgetrepocaps)
    extensions.wrapfunction(exchange, 'caps20to10', wrapcaps20to10)
    extensions.wrapfunction(exchange, '_canusebundle2', wrapcanusebundle2)
    extensions.wrapfunction(bundle2, 'bundle2caps', wrapbundle2caps)
    extensions.wrapfunction(bundle2, 'bundle20', wrapbundle20)

def wrapgetrepocaps(orig, repo, allowpushback=False):
    """re-register all bundle2 capabilities with a the "b2x" prefix"""
    caps = orig(repo, allowpushback)
    for key, value in caps.items():
        if not key.startswith('b2x:'):
            caps['b2x:' + key] = value
    return caps

def wrapcaps20to10(orig, repo):
    """advertise support for HG2Y and duplicate bundle2 capability in the
    generic "bundle2-exp" capability field."""
    caps = orig(repo)
    if 'HG20' in caps:
        caps.add('HG2Y')
    for c in caps:
        if c.startswith('bundle2='):
            caps.add(c.replace('bundle2=', 'bundle2-exp=', 1))
            break
    return caps

class bundle2y(bundle2.bundle20):
    """generate experimental header an prefix all parts names with b2x"""
    _magicstring = 'HG2Y'

    def newpart(self, typeid, *args, **kwargs):
        if not typeid.startswith('b2x:'):
            typeid = 'b2x:' + typeid
        return super(bundle2y, self).newpart(typeid, *args, **kwargs)

def wrapbundle20(orig, ui, b2caps=()):
    """use a HG2Y bundler if the remote only support that format"""
    if 'HG20' in b2caps:
        return orig(ui, b2caps)
    return bundle2y(ui, b2caps)

def wrapbundle2caps(orig, remote):
    """translate b2x: capability back to "normal" one

    this is necessary for the exchange logic to properly detect remote
    capability"""
    caps = orig(remote)
    if not caps:
        # search for experimental caps
        raw = remote.capable('bundle2-exp')
        if not (not raw and raw != ''):
            capsblob = urllib.unquote(remote.capable('bundle2-exp'))
            caps = bundle2.decodecaps(capsblob)
        # we need to trick the new code into thinking the old one support what we need.
        for key, value in caps.items():
            if key.startswith('b2x:'):
                caps[key.replace('b2x:', '', 1)] = value
    return caps

def wrapcanusebundle2(orig, op):
    """trigger bundle2 usage if the remote supports the experimental version"""
    return (op.repo.ui.configbool('experimental', 'bundle2-exp', False)
            and op.remote.capable('bundle2-exp')) or orig(op)
