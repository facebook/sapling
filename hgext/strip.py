from mercurial.i18n import _
from mercurial import cmdutil, hg, util
from mercurial.node import nullid
from mercurial.lock import release
from mercurial import repair

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

def checklocalchanges(repo, force=False, excsuffix=''):
    cmdutil.checkunfinished(repo)
    m, a, r, d = repo.status()[:4]
    if not force:
        if (m or a or r or d):
            _("local changes found") # i18n tool detection
            raise util.Abort(_("local changes found" + excsuffix))
        if checksubstate(repo):
            _("local changed subrepos found") # i18n tool detection
            raise util.Abort(_("local changed subrepos found" + excsuffix))
    return m, a, r, d

def strip(ui, repo, revs, update=True, backup="all", force=None):
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        if update:
            checklocalchanges(repo, force=force)
            urev, p2 = repo.changelog.parents(revs[0])
            if p2 != nullid and p2 in [x.node for x in repo.mq.applied]:
                urev = p2
            hg.clean(repo, urev)
            repo.dirstate.write()

        repair.strip(ui, repo, revs, backup)
    finally:
        release(lock, wlock)

