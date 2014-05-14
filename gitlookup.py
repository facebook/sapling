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

from mercurial import encoding, util, wireproto

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

def extsetup(ui):
    wrapwireprotocommand('lookup', remotelookup)

