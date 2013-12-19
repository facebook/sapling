# crecord.py
#
# Copyright 2008 Mark Edgington <edgimar@gmail.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.
#
# Much of this extension is based on Bryan O'Sullivan's record extension.

'''text-gui based change selection during commit or qrefresh'''
from mercurial.i18n import _
from mercurial import commands, extensions, util

from crecord_core import dorecord

def crecord(ui, repo, *pats, **opts):
    '''interactively select changes to commit

    If a list of files is omitted, all changes reported by :hg:`status`
    will be candidates for recording.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    You will be shown a list of patch hunks from which you can select
    those you would like to apply to the commit.

    This command is not available when committing a merge.'''

    dorecord(ui, repo, commands.commit, *pats, **opts)


def qcrecord(ui, repo, patch, *pats, **opts):
    '''interactively record a new patch

    See :hg:`help qnew` & :hg:`help crecord` for more information and
    usage.
    '''

    try:
        mq = extensions.find('mq')
    except KeyError:
        raise util.Abort(_("'mq' extension not loaded"))

    def committomq(ui, repo, *pats, **opts):
        mq.new(ui, repo, patch, *pats, **opts)

    opts = opts.copy()
    opts['force'] = True    # always 'qnew -f'
    dorecord(ui, repo, committomq, *pats, **opts)


def qcrefresh(ui, repo, *pats, **opts):
    """interactively update the current patch

    If any file patterns are provided, the refreshed patch will
    contain only the modifications that match those patterns; the
    remaining modifications will remain in the working directory.

    If -s/--short is specified, files currently included in the patch
    will be refreshed just like matched files and remain in the patch.

    hg add/remove/copy/rename work as usual, though you might want to
    use git-style patches (-g/--git or [diff] git=1) to track copies
    and renames. See the diffs help topic for more information on the
    git diff format.

    See :hg:`help qrefresh` & :hg:`help crecord` for more information and
    usage.
    """

    # Note: if the record operation (or subsequent refresh) fails partway
    # through, the top applied patch will be emptied and the working directory
    # will contain all of its changes.

    try:
        mq = extensions.find('mq')
    except KeyError:
        raise util.Abort(_("'mq' extension not loaded"))

    def refreshmq(ui, repo, *pats, **opts):
        mq.refresh(ui, repo, *pats, **opts)

    # Cannot use the simple pattern '*' because it will resolve relative to the
    # current working directory
    clearopts = { 'exclude': ["re:."], 'message': "" }

    mq.refresh(ui, repo, **clearopts)

    # if message wasn't specified in commandline, initialize from existing patch header
    if not opts.get('message',''):
        patchname = repo.mq.applied[-1].name
        patchmsg_lines = mq.patchheader(repo.mq.join(patchname), repo.mq.plainmode).message
        opts['message'] = '\n'.join(patchmsg_lines)

    dorecord(ui, repo, refreshmq, *pats, **opts)


cmdtable = {
    "crecord":
        (crecord,

         # add commit options
         commands.table['^commit|ci'][1],

         _('hg crecord [OPTION]... [FILE]...')),
}


def extsetup():
    try:
        keyword = extensions.find('keyword')
        keyword.restricted += ' crecord qcrecord qcrefresh'
        try:
            keyword.recordextensions += ' crecord'
            keyword.recordcommands += ' crecord qcrecord qcrefresh'
        except AttributeError:
            pass
    except KeyError:
        pass

    try:
        mq = extensions.find('mq')
    except KeyError:
        return

    qnew = '^qnew'
    if not qnew in mq.cmdtable:
        # backwards compatible with pre 301633755dec
        qnew = 'qnew'

    qrefresh = '^qrefresh'
    if not qrefresh in mq.cmdtable:
        # backwards compatible?
        qrefresh = 'qrefresh'

    qcmdtable = {
    "qcrecord":
        (qcrecord,

         # add qnew options, except '--force'
         [opt for opt in mq.cmdtable[qnew][1] if opt[1] != 'force'],

         _('hg qcrecord [OPTION]... PATCH [FILE]...')),

    "qcrefresh":
        (qcrefresh,

         # same options as qrefresh
         mq.cmdtable[qrefresh][1],

         _('hg qcrefresh [-I] [-X] [-e] [-m TEXT] [-l FILE] [-s] [FILE]...')),
    }

    cmdtable.update(qcmdtable)
