'''test helper extension to create commits with multiple extra fields'''

from mercurial import cmdutil, commands, scmutil

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

@command('commitextra',
         [('', 'field', [],
           'extra data to store', 'FIELD=VALUE'),
         ] + commands.commitopts + commands.commitopts2,
         'commitextra')
def commitextra(ui, repo, *pats, **opts):
    '''make a commit with extra fields'''
    fields = opts.get('field')
    extras = {}
    for field in fields:
        k, v = field.split('=', 1)
        extras[k] = v
    message = cmdutil.logmessage(ui, opts)
    repo.commit(message, opts.get('user'), opts.get('date'),
                match=scmutil.match(repo[None], pats, opts), extra=extras)
    return 0
