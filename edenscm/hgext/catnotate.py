# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from edenscm.mercurial import cmdutil, registrar, scmutil, util
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"


@command(
    "catnotate",
    [
        ("r", "rev", "", _("print the given revision"), _("REV")),
        ("a", "text", None, _("treat all files as text")),
    ],
    _("[OPTION]... FILE..."),
)
def catnotate(ui, repo, file1, *args, **opts):
    """output the current or given revision of files annotated with filename
    and line number.

    Print the specified files as they were at the given revision. If
    no revision is given, the parent of the working directory is used.

    Binary files are skipped unless -a/--text option is provided.
    """
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    matcher = scmutil.match(ctx, (file1,) + args, opts)
    prefix = ""

    err = 1
    # modified and stripped mercurial.cmdutil.cat follows
    def write(path):
        fp = cmdutil.makefileobj(
            repo, opts.get("output"), ctx.node(), pathname=os.path.join(prefix, path)
        )
        data = ctx[path].data()
        if not opts.get("text") and util.binary(data):
            fp.write("%s: binary file\n" % path)
            return

        for (num, line) in enumerate(data.split("\n"), start=1):
            line = line + "\n"
            fp.write("%s:%s: %s" % (path, num, line))
        fp.close()

    # Automation often uses hg cat on single files, so special case it
    # for performance to avoid the cost of parsing the manifest.
    if len(matcher.files()) == 1 and not matcher.anypats():
        file = matcher.files()[0]
        mfl = repo.manifestlog
        mfnode = ctx.manifestnode()
        if mfnode and mfl[mfnode].find(file)[0]:
            write(file)
            return 0

    for abs in ctx.walk(matcher):
        write(abs)
        err = 0

    return err
