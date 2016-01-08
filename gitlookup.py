# gitlookup.py - server-side support for hg->git and git->hg lookups
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# This will look up hashes from an hg-git map file over the wire. Define the
# location of the map file with the gitlookup.mapfile config option, then use
# thus:
#
# - get the git equivalent of hg 47d743e068523a9346a5ea4e429eeab185c886c6
#     hg identify --id -r _gitlookup_hg_47d743e068523a9346a5ea4e429eeab185c886c6 ssh://server/repo
# - get the hg equivalent of git 6916a3c30f53878032dea8d01074d8c2a03927bd
#     hg identify --id -r _gitlookup_git_6916a3c30f53878032dea8d01074d8c2a03927bd ssh://server/repo
#
# This also provides client and server commands to download all the Git metadata
# via bundle2.

from mercurial import bundle2, cmdutil, exchange, extensions, encoding, hg
from mercurial import util, wireproto, error
from mercurial.node import nullid
from mercurial.i18n import _
import errno, urllib

cmdtable = {}
command = cmdutil.command(cmdtable)

def wrapwireprotocommand(command, wrapper):
    '''Wrap the wire proto command named `command' in table

    Just like extensions.wrapcommand, except for wire protocol commands.
    '''
    assert util.safehasattr(wrapper, '__call__')
    origfn, args = wireproto.commands[command]
    def wrap(*args, **kwargs):
        return util.checksignature(wrapper)(
            util.checksignature(origfn), *args, **kwargs)
    wireproto.commands[command] = wrap, args
    return wrapper

def remotelookup(orig, repo, proto, key):
    k = encoding.tolocal(key)
    if k.startswith('_gitlookup_'):
        ret = _dolookup(repo, k)
        if ret is not None:
            success = 1
            return '%s %s\n' % (success, ret)
    return orig(repo, proto, key)

def _dolookup(repo, key):
    mapfile = repo.ui.configpath('gitlookup', 'mapfile')
    if mapfile is None:
        return None
    # direction: git to hg = g, hg to git = h
    if key.startswith('_gitlookup_git_'):
        direction = 'tohg'
        sha = key[15:]
    elif key.startswith('_gitlookup_hg_'):
        direction = 'togit'
        sha = key[14:]
    else:
        return None
    hggitmap = open(mapfile, 'rb')
    for line in hggitmap:
        gitsha, hgsha = line.strip().split(' ', 1)
        if direction == 'tohg' and sha == gitsha:
            return hgsha
        if direction == 'togit' and sha == hgsha:
            return gitsha
    return None

@command('gitgetmeta', [], '[SOURCE]')
def gitgetmeta(ui, repo, source='default'):
    '''get git metadata from a server that supports fb_gitmeta'''
    source, branch = hg.parseurl(ui.expandpath(source))
    other = hg.peer(repo, {}, source)
    ui.status(_('getting git metadata from %s\n') %
              util.hidepassword(source))
    kwargs = {'bundlecaps': exchange.caps20to10(repo)}
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo))
    kwargs['bundlecaps'].add('bundle2=' + urllib.quote(capsblob))
    # this would ideally not be in the bundlecaps at all, but adding new kwargs
    # for wire transmissions is not possible as of Mercurial d19164a018a1
    kwargs['bundlecaps'].add('fb_gitmeta')
    kwargs['heads'] = [nullid]
    kwargs['cg'] = False
    bundle = other.getbundle('pull', **kwargs)
    try:
        op = bundle2.processbundle(repo, bundle)
    except error.BundleValueError as exc:
        raise error.Abort('missing support for %s' % exc)
    writebytes = op.records['fb:gitmeta:writebytes']
    ui.status(_('wrote %d files (%d bytes)\n') %
              (len(writebytes), sum(writebytes)))

gitmetafiles = set(['git-mapfile', 'git-named-branches', 'git-tags',
                    'git-remote-refs'])

@exchange.getbundle2partsgenerator('b2x:fb:gitmeta')
def _getbundlegitmetapart(bundler, repo, source, bundlecaps=None, **kwargs):
    '''send git metadata via bundle2'''
    if 'fb_gitmeta' in bundlecaps:
        for fname in sorted(gitmetafiles):
            try:
                f = repo.opener(fname)
            except (IOError, OSError) as e:
                if e.errno != errno.ENOENT:
                    repo.ui.warn(_("warning: unable to read %s: %s\n") %
                                 (fname, e))
                continue
            part = bundle2.bundlepart('b2x:fb:gitmeta',
                                      [('filename', fname)],
                                      data=f.read())
            bundler.addpart(part)

@bundle2.parthandler('b2x:fb:gitmeta', ('filename',))
@bundle2.parthandler('fb:gitmeta', ('filename',))
def bundle2getgitmeta(op, part):
    '''unbundle a bundle2 containing git metadata on the client'''
    params = dict(part.mandatoryparams)
    if 'filename' not in params:
        raise error.Abort(_("gitmeta: 'filename' missing"))
    fname = params['filename']
    if fname not in gitmetafiles:
        ui.warn(_("warning: gitmeta: unknown file '%s' skipped\n") % fname)
        return
    f = op.repo.opener(fname, 'w+', atomictemp=True)
    try:
        data = part.read()
        op.repo.ui.note(_('writing .hg/%s\n') % fname)
        f.write(data)
        op.records.add('fb:gitmeta:writebytes', len(data))
    finally:
        f.close()

def extsetup(ui):
    wrapwireprotocommand('lookup', remotelookup)

