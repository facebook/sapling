# acl.py - changeset access control for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# this hook allows to allow or deny access to parts of a repo when
# taking incoming changesets.
#
# authorization is against local user name on system where hook is
# run, not committer of original changeset (since that is easy to
# spoof).
#
# acl hook is best to use if you use hgsh to set up restricted shells
# for authenticated users to only push to / pull from.  not safe if
# user has interactive shell access, because they can disable hook.
# also not safe if remote users share one local account, because then
# no way to tell remote users apart.
#
# to use, configure acl extension in hgrc like this:
#
#   [extensions]
#   hgext.acl =
#
#   [hooks]
#   pretxnchangegroup.acl = python:hgext.acl.hook
#
#   [acl]
#   sources = serve        # check if source of incoming changes in this list
#                          # ("serve" == ssh or http, "push", "pull", "bundle")
#
# allow and deny lists have subtree pattern (default syntax is glob)
# on left, user names on right. deny list checked before allow list.
#
#   [acl.allow]
#   # if acl.allow not present, all users allowed by default
#   # empty acl.allow = no users allowed
#   docs/** = doc_writer
#   .hgtags = release_engineer
#
#   [acl.deny]
#   # if acl.deny not present, no users denied by default
#   # empty acl.deny = all users allowed
#   glob pattern = user4, user5
#   ** = user6

from mercurial.i18n import _
from mercurial.node import bin, short
from mercurial import util
import getpass

class checker(object):
    '''acl checker.'''

    def buildmatch(self, key):
        '''return tuple of (match function, list enabled).'''
        if not self.ui.has_section(key):
            self.ui.debug(_('acl: %s not enabled\n') % key)
            return None, False

        thisuser = self.getuser()
        pats = [pat for pat, users in self.ui.configitems(key)
                if thisuser in users.replace(',', ' ').split()]
        self.ui.debug(_('acl: %s enabled, %d entries for user %s\n') %
                      (key, len(pats), thisuser))
        if pats:
            match = util.matcher(self.repo.root, names=pats)[1]
        else:
            match = util.never
        return match, True

    def getuser(self):
        '''return name of authenticated user.'''
        return self.user

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.user = getpass.getuser()
        cfg = self.ui.config('acl', 'config')
        if cfg:
            self.ui.readsections(cfg, 'acl.allow', 'acl.deny')
        self.allow, self.allowable = self.buildmatch('acl.allow')
        self.deny, self.deniable = self.buildmatch('acl.deny')

    def skipsource(self, source):
        '''true if incoming changes from this source should be skipped.'''
        ok_sources = self.ui.config('acl', 'sources', 'serve').split()
        return source not in ok_sources

    def check(self, node):
        '''return if access allowed, raise exception if not.'''
        files = self.repo.changectx(node).files()
        if self.deniable:
            for f in files:
                if self.deny(f):
                    self.ui.debug(_('acl: user %s denied on %s\n') %
                                  (self.getuser(), f))
                    raise util.Abort(_('acl: access denied for changeset %s') %
                                     short(node))
        if self.allowable:
            for f in files:
                if not self.allow(f):
                    self.ui.debug(_('acl: user %s not allowed on %s\n') %
                                  (self.getuser(), f))
                    raise util.Abort(_('acl: access denied for changeset %s') %
                                     short(node))
        self.ui.debug(_('acl: allowing changeset %s\n') % short(node))

def hook(ui, repo, hooktype, node=None, source=None, **kwargs):
    if hooktype != 'pretxnchangegroup':
        raise util.Abort(_('config error - hook type "%s" cannot stop '
                           'incoming changesets') % hooktype)

    c = checker(ui, repo)
    if c.skipsource(source):
        ui.debug(_('acl: changes have source "%s" - skipping\n') % source)
        return

    start = repo.changelog.rev(bin(node))
    end = repo.changelog.count()
    for rev in xrange(start, end):
        c.check(repo.changelog.node(rev))
