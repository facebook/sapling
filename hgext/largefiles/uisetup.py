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
from mercurial.hgweb import hgweb_mod, protocol, webcommands
from mercurial.subrepo import hgsubrepo

import overrides
import proto

def uisetup(ui):
    # Disable auto-status for some commands which assume that all
    # files in the result are under Mercurial's control

    entry = extensions.wrapcommand(commands.table, 'add',
                                   overrides.overrideadd)
    addopt = [('', 'large', None, _('add as largefile')),
              ('', 'normal', None, _('add as normal file')),
              ('', 'lfsize', '', _('add all files above this size '
                                   '(in megabytes) as largefiles '
                                   '(default: 10)'))]
    entry[1].extend(addopt)

    entry = extensions.wrapcommand(commands.table, 'addremove',
            overrides.overrideaddremove)
    entry = extensions.wrapcommand(commands.table, 'remove',
                                   overrides.overrideremove)
    entry = extensions.wrapcommand(commands.table, 'forget',
                                   overrides.overrideforget)

    # Subrepos call status function
    entry = extensions.wrapcommand(commands.table, 'status',
                                   overrides.overridestatus)
    entry = extensions.wrapfunction(hgsubrepo, 'status',
                                    overrides.overridestatusfn)

    entry = extensions.wrapcommand(commands.table, 'log',
                                   overrides.overridelog)
    entry = extensions.wrapcommand(commands.table, 'rollback',
                                   overrides.overriderollback)
    entry = extensions.wrapcommand(commands.table, 'verify',
                                   overrides.overrideverify)

    verifyopt = [('', 'large', None, _('verify largefiles')),
                 ('', 'lfa', None,
                     _('verify all revisions of largefiles not just current')),
                 ('', 'lfc', None,
                     _('verify largefile contents not just existence'))]
    entry[1].extend(verifyopt)

    entry = extensions.wrapcommand(commands.table, 'outgoing',
        overrides.overrideoutgoing)
    outgoingopt = [('', 'large', None, _('display outgoing largefiles'))]
    entry[1].extend(outgoingopt)
    entry = extensions.wrapcommand(commands.table, 'summary',
                                   overrides.overridesummary)
    summaryopt = [('', 'large', None, _('display outgoing largefiles'))]
    entry[1].extend(summaryopt)

    entry = extensions.wrapcommand(commands.table, 'update',
                                   overrides.overrideupdate)
    entry = extensions.wrapcommand(commands.table, 'pull',
                                   overrides.overridepull)
    entry = extensions.wrapcommand(commands.table, 'cat',
                                   overrides.overridecat)
    entry = extensions.wrapfunction(merge, '_checkunknownfile',
                                    overrides.overridecheckunknownfile)
    entry = extensions.wrapfunction(merge, 'manifestmerge',
                                    overrides.overridemanifestmerge)
    entry = extensions.wrapfunction(filemerge, 'filemerge',
                                    overrides.overridefilemerge)
    entry = extensions.wrapfunction(cmdutil, 'copy',
                                    overrides.overridecopy)

    # Summary calls dirty on the subrepos
    entry = extensions.wrapfunction(hgsubrepo, 'dirty',
                                    overrides.overridedirty)

    # Backout calls revert so we need to override both the command and the
    # function
    entry = extensions.wrapcommand(commands.table, 'revert',
                                   overrides.overriderevert)
    entry = extensions.wrapfunction(commands, 'revert',
                                    overrides.overriderevert)

    # clone uses hg._update instead of hg.update even though they are the
    # same function... so wrap both of them)
    extensions.wrapfunction(hg, 'update', overrides.hgupdate)
    extensions.wrapfunction(hg, '_update', overrides.hgupdate)
    extensions.wrapfunction(hg, 'clean', overrides.hgclean)
    extensions.wrapfunction(hg, 'merge', overrides.hgmerge)

    extensions.wrapfunction(archival, 'archive', overrides.overridearchive)
    extensions.wrapfunction(cmdutil, 'bailifchanged',
                            overrides.overridebailifchanged)

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

    extensions.wrapfunction(webcommands, 'decodepath', overrides.decodepath)

    # the hello wireproto command uses wireproto.capabilities, so it won't see
    # our largefiles capability unless we replace the actual function as well.
    proto.capabilitiesorig = wireproto.capabilities
    wireproto.capabilities = proto.capabilities

    # these let us reject non-largefiles clients and make them display
    # our error messages
    protocol.webproto.refuseclient = proto.webprotorefuseclient
    sshserver.sshserver.refuseclient = proto.sshprotorefuseclient

    # can't do this in reposetup because it needs to have happened before
    # wirerepo.__init__ is called
    proto.ssholdcallstream = sshrepo.sshrepository._callstream
    proto.httpoldcallstream = httprepo.httprepository._callstream
    sshrepo.sshrepository._callstream = proto.sshrepocallstream
    httprepo.httprepository._callstream = proto.httprepocallstream

    # don't die on seeing a repo with the largefiles requirement
    localrepo.localrepository.supported |= set(['largefiles'])

    # override some extensions' stuff as well
    for name, module in extensions.extensions():
        if name == 'fetch':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'fetch',
                overrides.overridefetch)
        if name == 'purge':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'purge',
                overrides.overridepurge)
        if name == 'rebase':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'rebase',
                overrides.overriderebase)
        if name == 'transplant':
            extensions.wrapcommand(getattr(module, 'cmdtable'), 'transplant',
                overrides.overridetransplant)
