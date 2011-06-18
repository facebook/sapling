# Extension dedicated to test patch.diff() upgrade modes
#
#
from mercurial import scmutil, patch, util

def autodiff(ui, repo, *pats, **opts):
    diffopts = patch.diffopts(ui, opts)
    git = opts.get('git', 'no')
    brokenfiles = set()
    losedatafn = None
    if git in ('yes', 'no'):
        diffopts.git = git == 'yes'
        diffopts.upgrade = False
    elif git == 'auto':
        diffopts.git = False
        diffopts.upgrade = True
    elif git == 'warn':
        diffopts.git = False
        diffopts.upgrade = True
        def losedatafn(fn=None, **kwargs):
            brokenfiles.add(fn)
            return True
    elif git == 'abort':
        diffopts.git = False
        diffopts.upgrade = True
        def losedatafn(fn=None, **kwargs):
            raise util.Abort('losing data for %s' % fn)
    else:
        raise util.Abort('--git must be yes, no or auto')

    node1, node2 = scmutil.revpair(repo, [])
    m = scmutil.match(repo[node2], pats, opts)
    it = patch.diff(repo, node1, node2, match=m, opts=diffopts,
                    losedatafn=losedatafn)
    for chunk in it:
        ui.write(chunk)
    for fn in sorted(brokenfiles):
        ui.write('data lost for: %s\n' % fn)

cmdtable = {
    "autodiff":
        (autodiff,
         [('', 'git', '', 'git upgrade mode (yes/no/auto/warn/abort)'),
          ],
         '[OPTION]... [FILE]...'),
}
