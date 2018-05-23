# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

from __future__ import absolute_import
import code
import sys
import mercurial

from mercurial.i18n import _
from mercurial import (
    demandimport,
    registrar,
)

cmdtable = {}
command = registrar.command(cmdtable)

def _assignobjects(objects, repo):
    objects.update({
        'm': mercurial,
        'mercurial': mercurial,
    })
    if repo:
        objects.update({
            'repo': repo,
            'cl': repo.changelog,
            'mf': repo.manifestlog,
        })

def pdb(ui, repo, msg, **opts):
    objects = {}
    _assignobjects(objects, repo)
    code.interact(msg, local=objects)

def ipdb(ui, repo, msg, **opts):
    import IPython
    _assignobjects(locals(), repo)
    IPython.embed()

@command('debugshell|dbsh', [
    ('c', 'command', '', _('program passed in as string'), _('CMD'))
], optionalrepo=True)
def debugshell(ui, repo, **opts):
    command = opts.get('command')
    if command:
        _assignobjects(locals(), repo)
        exec(command)
        return 0

    bannermsg = "loaded repo : %s\n" \
                "using source: %s" % (repo and repo.root or '(none)',
                                      mercurial.__path__[0])

    pdbmap = {
        'pdb'  : 'code',
        'ipdb' : 'IPython'
    }

    debugger = ui.config("ui", "debugger")
    if not debugger:
        debugger = 'pdb'

    # if IPython doesn't exist, fallback to code.interact
    try:
        with demandimport.deactivated():
            __import__(pdbmap[debugger])
    except ImportError:
        ui.warn(("%s debugger specified but %s module was not found\n")
                % (debugger, pdbmap[debugger]))
        debugger = 'pdb'

    getattr(sys.modules[__name__], debugger)(ui, repo, bannermsg, **opts)
