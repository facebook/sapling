from mercurial import cmdutil, context, error, registrar
from mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)


@command("debugcommitmessage", [], _("FORM"))
def debugcommitmessage(ui, repo, *args):
    form = None
    if len(args) > 1:
        raise error.Abort(_("provide at most one form"))
    elif len(args) > 0:
        form = args[0]

    status = repo.status()
    text = ""
    user = None
    date = None
    extra = None

    ctx = context.workingcommitctx(repo, status, text, user, date, extra)

    subs = []
    editform = form or "commit.normal.normal"
    extramsg = _("Leave message empty to abort commit.")

    forms = [e for e in editform.split(".") if e]
    forms.insert(0, "changeset")
    while forms:
        ref = ".".join(forms)
        tmpl = repo.ui.config("committemplate", ref)
        if tmpl:
            committext = cmdutil.buildcommittemplate(repo, ctx, subs, extramsg, ref)
            break
        forms.pop()
    else:
        committext = cmdutil.buildcommittext(repo, ctx, subs, extramsg)

    ui.status(committext)
