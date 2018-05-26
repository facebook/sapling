from mercurial import registrar
from mercurial.node import hex, nullid


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"


@command("whereami")
def whereami(ui, repo, *args, **opts):
    """output the current working directory parents

    Outputs the hashes of current working directory parents separated
    by newline.
    """
    parents = repo.dirstate.parents()
    ui.status("%s\n" % hex(parents[0]))
    if parents[1] != nullid:
        ui.status("%s\n" % hex(parents[1]))
