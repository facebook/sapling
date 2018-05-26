# fbshow.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""show changesets in detail with full log message, patches etc

For example, 'hg show' prints something like
::

  $ hg show
  changeset:   1:b73358b94785
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  longer

  diff -r 852a8d467a01 -r b73358b94785 x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   show
  +more

and 'hg show --stat' prints something like:

  $ hg show --stat
  changeset:   1:b73358b94785
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x
  description:
  longer

   x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

"""

from mercurial import cmdutil, commands, error, registrar, scmutil
from mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"


def uisetup(ui):
    permitted_opts = (
        [
            ("g", "git", None, _("use git extended diff format")),
            ("", "stat", None, _("output diffstat-style summary of changes")),
        ]
        + commands.templateopts
        + commands.walkopts
    )

    local_opts = [
        (
            "",
            "nodates",
            None,
            _("omit dates from diff headers " + "(but keeps it in commit header)"),
        ),
        ("", "noprefix", None, _("omit a/ and b/ prefixes from filenames")),
        ("U", "unified", int, _("number of lines of diff context to show")),
    ] + commands.diffwsopts

    aliases, entry = cmdutil.findcmd("log", commands.table)
    allowed_opts = [opt for opt in entry[1] if opt in permitted_opts] + local_opts

    # manual call of the decorator
    command("^show", allowed_opts, _("[OPTION]... [REV [FILE]...]"), inferrepo=True)(
        show
    )


def show(ui, repo, *args, **opts):
    """show revision in detail

    This behaves similarly to :hg:`log -vp -r REV [OPTION]... [FILE]...`, or
    if called without a REV, :hg:`log -vp -r . [OPTION]...` Use
    :hg:`log` for more powerful operations than supported by hg show

    See :hg:`help templates` for more about pre-packaged styles and
    specifying custom templates.

    """
    ui.pager("show")
    if len(args) == 0:
        opts["rev"] = ["."]
        pats = []
    else:
        opts["rev"] = [args[0]]
        pats = args[1:]
        if not scmutil.revrange(repo, opts["rev"]):
            h = _("if %s is a file, try `hg show . %s`") % (args[0], args[0])
            raise error.Abort(_("unknown revision %s") % args[0], hint=h)

    opts["patch"] = not opts["stat"]
    opts["verbose"] = True

    # Copy tracking is slow when doing a git diff. Override hgrc, and rely on
    # opts getting us a git diff if it's been requested. Ideally, we'd find and
    # fix the slowness in copy tracking, but this works for now.
    # On a commit with lots of possible copies, Bryan O'Sullivan found that this
    # reduces "time hg show" from 1.76 seconds to 0.81 seconds.
    overrides = {
        ("diff", "git"): opts.get("git"),
        ("diff", "unified"): opts.get("unified"),
        ("ui", "verbose"): True,
    }
    overrides.update({("diff", opt): opts.get(opt) for opt in commands.diffwsopts})

    logcmd, defaultlogopts = cmdutil.getcmdanddefaultopts("log", commands.table)
    defaultlogopts.update(opts)

    with ui.configoverride(overrides, "show"):
        logcmd(ui, repo, *pats, **defaultlogopts)
