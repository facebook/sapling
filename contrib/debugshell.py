# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

import mercurial
import code

def debugshell(ui, repo, **opts):
    objects = {
        'mercurial': mercurial,
        'repo': repo,
        'cl': repo.changelog,
        'mf': repo.manifest,
    }
    bannermsg = "loaded repo : %s\n" \
                "using source: %s" % (repo.root,
                                      mercurial.__path__[0])
    code.interact(bannermsg, local=objects)

cmdtable = {
    "debugshell|dbsh": (debugshell, [])
}
