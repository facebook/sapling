# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

import sys
import mercurial
import code

def pdb(ui, repo, msg, **opts):
    objects = {
        'mercurial': mercurial,
        'repo': repo,
        'cl': repo.changelog,
        'mf': repo.manifest,
    }

    code.interact(msg, local=objects)

def ipdb(ui, repo, msg, **opts):
    import IPython

    cl = repo.changelog
    mf = repo.manifest
    cl, mf # use variables to appease pyflakes

    IPython.embed()

def debugshell(ui, repo, **opts):
    bannermsg = "loaded repo : %s\n" \
                "using source: %s" % (repo.root,
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
        __import__(pdbmap[debugger])
    except ImportError:
        ui.warn("%s debugger specified but %s module was not found\n"
                % (debugger, pdbmap[debugger]))
        debugger = 'pdb'

    getattr(sys.modules[__name__], debugger)(ui, repo, bannermsg, **opts)

cmdtable = {
    "debugshell|dbsh": (debugshell, [])
}
