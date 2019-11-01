# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fastannotate: faster annotate implementation using linelog

"""yet another annotate implementation that might be faster

The fastannotate extension provides a 'fastannotate' command that makes
use of the linelog data structure as a cache layer and is expected to
be faster than the vanilla 'annotate' if the cache is present.

In most cases, fastannotate requires a setup that mainbranch is some pointer
that always moves forward, to be most efficient.

Using fastannotate together with linkrevcache would speed up building the
annotate cache greatly. Run "debugbuildlinkrevcache" before
"debugbuildannotatecache".

::

    [fastannotate]
    # specify the main branch head. the internal linelog will only contain
    # the linear (ignoring p2) "mainbranch". since linelog cannot move
    # backwards without a rebuild, this should be something that always moves
    # forward, usually it is "master" or "@".
    mainbranch = master

    # fastannotate supports different modes to expose its feature.
    # a list of combination:
    # - fastannotate: expose the feature via the "fastannotate" command which
    #   deals with everything in a most efficient way, and provides extra
    #   features like --deleted etc.
    # - fctx: replace fctx.annotate implementation. note:
    #     a. it is less efficient than the "fastannotate" command
    #     b. it will make it practically impossible to access the old (disk
    #        side-effect free) annotate implementation
    #     c. it implies "hgweb".
    # - hgweb: replace hgweb's annotate implementation. conflict with "fctx".
    # (default: fastannotate)
    modes = fastannotate

    # default format when no format flags are used (default: number)
    defaultformat = changeset, user, date

    # serve the annotate cache via wire protocol (default: False)
    # tip: the .hg/fastannotate directory is portable - can be rsynced
    server = True

    # build annotate cache on demand for every client request (default: True)
    # disabling it could make server response faster, useful when there is a
    # cronjob building the cache.
    serverbuildondemand = True

    # update local annotate cache from remote on demand
    # (default: True for remotefilelog repo, False otherwise)
    client = True

    # path to use when connecting to the remote server (default: default)
    remotepath = default

    # share sshpeer with remotefilelog. this would allow fastannotate to peek
    # into remotefilelog internals, and steal its sshpeer, or in the reversed
    # direction: donate its sshpeer to remotefilelog. disable this if
    # fastannotate and remotefilelog should not share a sshpeer when their
    # endpoints are different and incompatible. (default: True)
    clientsharepeer = True

    # minimal length of the history of a file required to fetch linelog from
    # the server. (default: 10)
    clientfetchthreshold = 10

    # use flock instead of the file existence lock
    # flock may not work well on some network filesystems, but they avoid
    # creating and deleting files frequently, which is faster when updating
    # the annotate cache in batch. if you have issues with this option, set it
    # to False. (default: True if flock is supported, False otherwise)
    useflock = True

    # for "fctx" mode, always follow renames regardless of command line option.
    # this is a BC with the original command but will reduced the space needed
    # for annotate cache, and is useful for client-server setup since the
    # server will only provide annotate cache with default options (i.e. with
    # follow). do not affect "fastannotate" mode. (default: False)
    forcefollow = False

    # for "fctx" mode, always treat file as text files, to skip the "isbinary"
    # check. this is consistent with the "fastannotate" command and could help
    # to avoid a file fetch if remotefilelog is used. (default: True)
    forcetext = True

    # use unfiltered repo for better performance.
    unfilteredrepo = True

    # sacrifice correctness in some corner cases for performance. it does not
    # affect the correctness of the annotate cache being built. the option
    # is experimental and may disappear in the future (default: False)
    perfhack = True
"""

from __future__ import absolute_import

from edenscm.mercurial import error as hgerror, localrepo, util
from edenscm.mercurial.i18n import _

from . import commands, context, protocol


testedwith = "ships-with-fb-hgext"

cmdtable = commands.cmdtable


def _flockavailable():
    try:
        import fcntl

        fcntl.flock
    except Exception:
        return False
    else:
        return True


def uisetup(ui):
    modes = set(ui.configlist("fastannotate", "modes", ["fastannotate"]))
    if "fctx" in modes:
        modes.discard("hgweb")
    for name in modes:
        if name == "fastannotate":
            commands.registercommand()
        elif name == "hgweb":
            from . import support

            support.replacehgwebannotate()
        elif name == "fctx":
            from . import support

            support.replacefctxannotate()
            support.replaceremotefctxannotate()
            commands.wrapdefault()
        else:
            raise hgerror.Abort(_("fastannotate: invalid mode: %s") % name)

    if ui.configbool("fastannotate", "server"):
        protocol.serveruisetup(ui)

    if ui.configbool("fastannotate", "useflock", _flockavailable()):
        context.pathhelper.lock = context.pathhelper._lockflock

    # fastannotate has its own locking, without depending on repo lock
    localrepo.localrepository._wlockfreeprefix.add("fastannotate/")


def reposetup(ui, repo):
    client = ui.configbool("fastannotate", "client", default=None)
    if client is None:
        if util.safehasattr(repo, "requirements"):
            client = "remotefilelog" in repo.requirements
    if client:
        protocol.clientreposetup(ui, repo)
