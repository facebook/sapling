from mercurial.i18n import _
from mercurial import cmdutil, util

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def checksubstate(repo, baserev=None):
    '''return list of subrepos at a different revision than substate.
    Abort if any subrepos have uncommitted changes.'''
    inclsubs = []
    wctx = repo[None]
    if baserev:
        bctx = repo[baserev]
    else:
        bctx = wctx.parents()[0]
    for s in sorted(wctx.substate):
        if wctx.sub(s).dirty(True):
            raise util.Abort(
                _("uncommitted changes in subrepository %s") % s)
        elif s not in bctx.substate or bctx.sub(s).dirty():
            inclsubs.append(s)
    return inclsubs

