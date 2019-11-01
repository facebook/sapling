# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import random
import time
import traceback

from edenscm.mercurial import context, error, registrar, scmutil
from edenscm.mercurial.i18n import _

from ..perfsuite import editsgenerator


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-hgext"


def _mainloop(repo, ui, tr, count, batchsize):
    i = 0
    batchcount = 0
    start = time.time()
    base = scmutil.revsingle(repo, ui.config("repogenerator", "startcommit", "tip"))
    goalrev = ui.configint("repogenerator", "numcommits", 10000) - 1
    ui.write(_("starting commit is: %d (goal is %d)\n") % (base.rev(), goalrev))
    generator = editsgenerator.randomeditsgenerator(base)

    while base.rev() < goalrev:
        # Make a commit:
        wctx = context.overlayworkingctx(repo)
        wctx.setbase(base)
        generator.makerandomedits(wctx)
        memctx = wctx.tomemctx("memory commit", parents=(base.rev(), None))
        newctx = repo[repo.commitctx(memctx)]

        # Log production rate:
        elapsed = time.time() - start
        if i % 5 == 0:
            ui.write(
                _(
                    "created %s, %0.2f sec elapsed "
                    "(%0.2f commits/sec, %s per hour, %s per day)\n"
                )
                % (
                    i,
                    elapsed,
                    i / elapsed,
                    "{:,}".format(int(i / elapsed * 3600)),
                    "{:,}".format(int(i / elapsed * 86400)),
                )
            )
        base = newctx
        i += 1
        batchcount += 1
        if batchcount > batchsize:
            ui.status(_("committing txn...\n"))
            tr.close()
            tr = repo.transaction("newtxn_")
            batchcount = 0
        if i >= count:
            ui.status(_("generated %d commits; quitting\n") % count)
            return


@command(
    "repogenerator",
    [
        ("", "batch-size", 50000, _("size of transactiions to commit")),
        ("n", "count", 50000, _("number of commits to generate")),
        ("", "seed", 0, _("random seed to use")),
    ],
    _("hg repogenerator [OPTION] [REV]"),
)
def repogenerator(ui, repo, *revs, **opts):
    """Generates random commits for large-scale repo generation

    The starting revision is configurable::

       [repogenerator]
       startcommit = tip

    The number of commits is configurable::

        [repogenerator]
        numcommits = 10000

    The shape of generated paths can be tweaked::

        [repogenerator]
        filenamedircount = 3
        filenameleaflength = 3
    """
    if opts["seed"]:
        random.seed(opts["seed"])

    with repo.wlock(), repo.lock():
        try:
            tr = repo.transaction("")
        except error.AbandonedTransactionFoundError:
            ui.status(_("recovering abandoned transaction...\n"))
            repo.recover()
            tr = repo.transaction("")

        try:
            _mainloop(repo, ui, tr, opts["count"], opts["batch_size"])
            tr.close()
        except KeyboardInterrupt:
            ui.status(_("interrupted...\n"))
            tr.abort()
        except Exception:
            traceback.print_exc()
            tr.abort()
            raise
