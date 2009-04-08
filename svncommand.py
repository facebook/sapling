import os
import sys
import traceback

from util import svn_subcommands, svn_commands_nourl
# dirty trick to force demandimport to run my decorator anyway.
from svncommands import pull, diff, rebuildmeta
from utility_commands import print_wc_url
# shut up, pyflakes, we must import those
__x = [print_wc_url, pull, diff, rebuildmeta]


def svncmd(ui, repo, subcommand, *args, **opts):
    if subcommand not in svn_subcommands:
        candidates = []
        for c in svn_subcommands:
            if c.startswith(subcommand):
                candidates.append(c)
        if len(candidates) == 1:
            subcommand = candidates[0]
    path = os.path.dirname(repo.path)
    try:
        commandfunc = svn_subcommands[subcommand]
        if commandfunc not in svn_commands_nourl:
            opts['svn_url'] = open(os.path.join(repo.path, 'svn', 'url')).read()
        return commandfunc(ui, args=args,
                           hg_repo_path=path,
                           repo=repo,
                           **opts)
    except TypeError:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Bad arguments for subcommand %s\n' % subcommand)
        else:
            raise
    except KeyError, e:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Unknown subcommand %s\n' % subcommand)
        else:
            raise
