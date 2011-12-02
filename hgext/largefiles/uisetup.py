# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''setup for largefiles extension: uisetup'''

from mercurial import archival, cmdutil, commands, extensions, filemerge, hg, \
    httprepo, localrepo, merge, sshrepo, sshserver, wireproto
from mercurial.i18n import _
from mercurial.hgweb import hgweb_mod, protocol

import overrides
import proto

def uisetup(ui):
    # Disable auto-status for some commands which assume that all
    # files in the result are under Mercurial's control

    entry = extensions.wrapcommand(commands.table, 'add',
                                   overrides.override_add)
    addopt = [('', 'large', None, _('add as largefile')),
              ('', 'normal', None, _('add as normal file')),
              ('', 'lfsize', '', _('add all files above this size '
                                   '(in megabytes) as largefiles '
                                   '(default: 10)'))]
    entry[1].extend(addopt)

    entry = extensions.wrapcommand(commands.table, 'addremove',
            overrides.override_addremove)
    entry = extensions.wrapcommand(commands.table, 'remove',
                                   overrides.override_remove)
    entry = extensions.wrapcommand(commands.table, 'forget',
                                   overrides.override_forget)
    entry = extensions.wrapcommand(commands.table, 'status',
                                   overrides.override_status)
    entry = extensions.wrapcommand(commands.table, 'log',
                                   overrides.override_log)
    entry = extensions.wrapcommand(commands.table, 'rollback',
                                   overrides.override_rollback)
    entry = extensions.wrapcommand(commands.table, 'verify',
                                   overrides.override_verify)

    verifyopt = [('', 'large', None, _('verify largefiles')),
                 ('', 'lfa', None,
                     _('verify all revisions of largefiles not just current')),
                 ('', 'lfc', None,
                     _('verify largefile contents not just existence'))]
    entry[1].extend(verifyopt)

    entry = extensions.wrapcommand(commands.table, 'outgoing',
        overrides.override_outgoing)
    outgoingopt = [('', 'large', None, _('display outgoing largefiles'))]
    entry[1].extend(outgoingopt)
    entry = extensions.wrapcommand(commands.table, 'summary',
                                   overrides.override_summary)
    summaryopt = [('', 'large', None, _('display outgoing largefiles'))]
    entry[1].extend(summaryopt)

    entry = extensions.wrapcommand(commands.table, 'update',
                                   overrides.override_update)
    entry = extensions.wrapcommand(commands.table, 'pull',
                                   overrides.override_pull)
    entry = extensions.wrapfunction(merge, '_checkunknown',
                                    overrides.override_checkunknown)
    entry = extensions.wrapfunction(merge, 'manifestmerge',
                                    overrides.override_manifestmerge)
    entry = extensions.wrapfunction(filemerge, 'filemerge',
                                    overrides.override_filemerge)
    entry = extensions.wrapfunction(cmdutil, 'copy',
                                    overrides.override_copy)

    # Backout calls revert so we need to override both the command and the
    # function
    entry = extensions.wrapcommand(commands.table, 'revert',
                                   overrides.override_revert)
    entry = extensions.wrapfunction(commands, 'revert',
                                    overrides.override_revert)

    # clone uses hg._update instead of hg.update even though they are the
    # same function... so wrap both of them)
    extensions.wrapfunction(hg, 'update', overrides.hg_update)
    extensions.wrapfunction(hg, '_update', overrides.hg_update)
    extensions.wrapfunction(hg, 'clean', overrides.hg_clean)
    extensions.wrapfunction(hg, 'merge', overrides.hg_merge)

    extensions.wrapfunction(archival, 'archive', overrides.override_archive)
    extensions.wrapfunction(cmdutil, 'bailifchanged',
                            overrides.override_bailifchanged)

    # create the new wireproto commands ...
    wireproto.commands['putlfile'] = (proto.putlfile, 'sha')
    wireproto.commands['getlfile'] = (proto.getlfile, 'sha')
    wireproto.commands['statlfile'] = (proto.statlfile, 'sha')

    # ... and wrap some existing ones
    wireproto.commands['capabilities'] = (proto.capabilities, '')
    wireproto.commands['heads'] = (proto.heads, '')
    wireproto.commands['lheads'] = (wireproto.heads, '')

    # make putlfile behave the same as push and {get,stat}lfile behave
    # the same as pull w.r.t. permissions checks
    hgweb_mod.perms['putlfile'] = 'push'
    hgweb_mod.perms['getlfile'] = 'pull'
    hgweb_mod.perms['statlfile'] = 'pull'

    # the hello wireproto command uses wireproto.capabilities, so it won't see
    # our largefiles capability unless we replace the actual function as well.
    proto.capabilities_orig = wireproto.capabilities
    wireproto.capabilities = proto.capabilities

    # these let us reject non-largefiles clients and make them display
    # our error messages
    protocol.webproto.refuseclient = proto.webproto_refuseclient
    sshserver.sshserver.refuseclient = proto.sshproto_refuseclient

    # can't do this in reposetup because it needs to have happened before
    # wirerepo.__init__ is called
    proto.ssh_oldcallstream = sshrepo.sshrepository._callstream
    proto.http_oldcallstream = httprepo.httprepository._callstream
    sshrepo.sshrepository._callstream = proto.sshrepo_callstream
    httprepo.httprepository._callstream = proto.httprepo_callstream

    # don't die on seeing a repo with the largefiles requirement
    localrepo.localrepository.supported |= set(['largefiles'])

    # override some extensions' stuff as well
    for name, module in extensions.extensions():
        if name == 'fetch':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'fetch',
                overrides.override_fetch)
        if name == 'purge':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'purge',
                overrides.override_purge)
        if name == 'rebase':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'rebase',
                overrides.override_rebase)
        if name == 'transplant':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'transplant',
                overrides.override_transplant)
