# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

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

def debugshell(ui, repo, **opts):
    bannermsg = "loaded repo : %s\n" \
                "using source: %s" % (repo.root,
                                      mercurial.__path__[0])

    pdb(ui, repo, bannermsg, **opts)

cmdtable = {
    "debugshell|dbsh": (debugshell, [])
}
