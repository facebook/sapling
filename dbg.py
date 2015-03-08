from mercurial import cmdutil
import pdb

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

@command('dbg', [])
def debug_(ui, repo):
    """
    Open up a python debugger within a command context
    """
    pdb.set_trace()
