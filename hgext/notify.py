# notify.py - email notifications for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# hook extension to email notifications to people when changesets are
# committed to a repo they subscribe to.
#
# default mode is to print messages to stdout, for testing and
# configuring.
#
# to use, configure notify extension and enable in hgrc like this:
#
#   [extensions]
#   hgext.notify =
#
#   [hooks]
#   # one email for each incoming changeset
#   incoming.notify = python:hgext.notify.hook
#   # batch emails when many changesets incoming at one time
#   changegroup.notify = python:hgext.notify.hook
#
#   [notify]
#   # config items go in here
#
# config items:
#
# REQUIRED:
#   config = /path/to/file # file containing subscriptions
#
# OPTIONAL:
#   test = True            # print messages to stdout for testing
#   strip = 3              # number of slashes to strip for url paths
#   domain = example.com   # domain to use if committer missing domain
#   style = ...            # style file to use when formatting email
#   template = ...         # template to use when formatting email
#   incoming = ...         # template to use when run as incoming hook
#   changegroup = ...      # template when run as changegroup hook
#   maxdiff = 300          # max lines of diffs to include (0=none, -1=all)
#   maxsubject = 67        # truncate subject line longer than this
#   diffstat = True        # add a diffstat before the diff content
#   sources = serve        # notify if source of incoming changes in this list
#                          # (serve == ssh or http, push, pull, bundle)
#   [email]
#   from = user@host.com   # email address to send as if none given
#   [web]
#   baseurl = http://hgserver/... # root of hg web site for browsing commits
#
# notify config file has same format as regular hgrc. it has two
# sections so you can express subscriptions in whatever way is handier
# for you.
#
#   [usersubs]
#   # key is subscriber email, value is ","-separated list of glob patterns
#   user@host = pattern
#
#   [reposubs]
#   # key is glob pattern, value is ","-separated list of subscriber emails
#   pattern = user@host
#
# glob patterns are matched against path to repo root.
#
# if you like, you can put notify config file in repo that users can
# push changes to, they can manage their own subscriptions.

from mercurial.i18n import _
from mercurial.node import bin, short
from mercurial import patch, cmdutil, templater, util, mail
import email.Parser, fnmatch, socket, time

# template for single changeset can include email headers.
single_template = '''
Subject: changeset in {webroot}: {desc|firstline|strip}
From: {author}

changeset {node|short} in {root}
details: {baseurl}{webroot}?cmd=changeset;node={node|short}
description:
\t{desc|tabindent|strip}
'''.lstrip()

# template for multiple changesets should not contain email headers,
# because only first set of headers will be used and result will look
# strange.
multiple_template = '''
changeset {node|short} in {root}
details: {baseurl}{webroot}?cmd=changeset;node={node|short}
summary: {desc|firstline}
'''

deftemplates = {
    'changegroup': multiple_template,
}

class notifier(object):
    '''email notification class.'''

    def __init__(self, ui, repo, hooktype):
        self.ui = ui
        cfg = self.ui.config('notify', 'config')
        if cfg:
            self.ui.readsections(cfg, 'usersubs', 'reposubs')
        self.repo = repo
        self.stripcount = int(self.ui.config('notify', 'strip', 0))
        self.root = self.strip(self.repo.root)
        self.domain = self.ui.config('notify', 'domain')
        self.subs = self.subscribers()

        mapfile = self.ui.config('notify', 'style')
        template = (self.ui.config('notify', hooktype) or
                    self.ui.config('notify', 'template'))
        self.t = cmdutil.changeset_templater(self.ui, self.repo,
                                             False, mapfile, False)
        if not mapfile and not template:
            template = deftemplates.get(hooktype) or single_template
        if template:
            template = templater.parsestring(template, quoted=False)
            self.t.use_template(template)

    def strip(self, path):
        '''strip leading slashes from local path, turn into web-safe path.'''

        path = util.pconvert(path)
        count = self.stripcount
        while count > 0:
            c = path.find('/')
            if c == -1:
                break
            path = path[c+1:]
            count -= 1
        return path

    def fixmail(self, addr):
        '''try to clean up email addresses.'''

        addr = util.email(addr.strip())
        if self.domain:
            a = addr.find('@localhost')
            if a != -1:
                addr = addr[:a]
            if '@' not in addr:
                return addr + '@' + self.domain
        return addr

    def subscribers(self):
        '''return list of email addresses of subscribers to this repo.'''

        subs = {}
        for user, pats in self.ui.configitems('usersubs'):
            for pat in pats.split(','):
                if fnmatch.fnmatch(self.repo.root, pat.strip()):
                    subs[self.fixmail(user)] = 1
        for pat, users in self.ui.configitems('reposubs'):
            if fnmatch.fnmatch(self.repo.root, pat):
                for user in users.split(','):
                    subs[self.fixmail(user)] = 1
        subs = subs.keys()
        subs.sort()
        return subs

    def url(self, path=None):
        return self.ui.config('web', 'baseurl') + (path or self.root)

    def node(self, node):
        '''format one changeset.'''

        self.t.show(changenode=node, changes=self.repo.changelog.read(node),
                    baseurl=self.ui.config('web', 'baseurl'),
                    root=self.repo.root,
                    webroot=self.root)

    def skipsource(self, source):
        '''true if incoming changes from this source should be skipped.'''
        ok_sources = self.ui.config('notify', 'sources', 'serve').split()
        return source not in ok_sources

    def send(self, node, count, data):
        '''send message.'''

        p = email.Parser.Parser()
        msg = p.parsestr(data)

        def fix_subject():
            '''try to make subject line exist and be useful.'''

            subject = msg['Subject']
            if not subject:
                if count > 1:
                    subject = _('%s: %d new changesets') % (self.root, count)
                else:
                    changes = self.repo.changelog.read(node)
                    s = changes[4].lstrip().split('\n', 1)[0].rstrip()
                    subject = '%s: %s' % (self.root, s)
            maxsubject = int(self.ui.config('notify', 'maxsubject', 67))
            if maxsubject and len(subject) > maxsubject:
                subject = subject[:maxsubject-3] + '...'
            del msg['Subject']
            msg['Subject'] = subject

        def fix_sender():
            '''try to make message have proper sender.'''

            sender = msg['From']
            if not sender:
                sender = self.ui.config('email', 'from') or self.ui.username()
            if '@' not in sender or '@localhost' in sender:
                sender = self.fixmail(sender)
            del msg['From']
            msg['From'] = sender

        msg['Date'] = util.datestr(format="%a, %d %b %Y %H:%M:%S %1%2")
        fix_subject()
        fix_sender()

        msg['X-Hg-Notification'] = 'changeset ' + short(node)
        if not msg['Message-Id']:
            msg['Message-Id'] = ('<hg.%s.%s.%s@%s>' %
                                 (short(node), int(time.time()),
                                  hash(self.repo.root), socket.getfqdn()))
        msg['To'] = ', '.join(self.subs)

        msgtext = msg.as_string(0)
        if self.ui.configbool('notify', 'test', True):
            self.ui.write(msgtext)
            if not msgtext.endswith('\n'):
                self.ui.write('\n')
        else:
            self.ui.status(_('notify: sending %d subscribers %d changes\n') %
                           (len(self.subs), count))
            mail.sendmail(self.ui, util.email(msg['From']),
                          self.subs, msgtext)

    def diff(self, node, ref):
        maxdiff = int(self.ui.config('notify', 'maxdiff', 300))
        prev = self.repo.changelog.parents(node)[0]
        self.ui.pushbuffer()
        patch.diff(self.repo, prev, ref)
        difflines = self.ui.popbuffer().splitlines(1)
        if self.ui.configbool('notify', 'diffstat', True):
            s = patch.diffstat(difflines)
            # s may be nil, don't include the header if it is
            if s:
                self.ui.write('\ndiffstat:\n\n%s' % s)
        if maxdiff == 0:
            return
        if maxdiff > 0 and len(difflines) > maxdiff:
            self.ui.write(_('\ndiffs (truncated from %d to %d lines):\n\n') %
                          (len(difflines), maxdiff))
            difflines = difflines[:maxdiff]
        elif difflines:
            self.ui.write(_('\ndiffs (%d lines):\n\n') % len(difflines))
        self.ui.write(*difflines)

def hook(ui, repo, hooktype, node=None, source=None, **kwargs):
    '''send email notifications to interested subscribers.

    if used as changegroup hook, send one email for all changesets in
    changegroup. else send one email per changeset.'''
    n = notifier(ui, repo, hooktype)
    if not n.subs:
        ui.debug(_('notify: no subscribers to repo %s\n') % n.root)
        return
    if n.skipsource(source):
        ui.debug(_('notify: changes have source "%s" - skipping\n') %
                 source)
        return
    node = bin(node)
    ui.pushbuffer()
    if hooktype == 'changegroup':
        start = repo.changelog.rev(node)
        end = repo.changelog.count()
        count = end - start
        for rev in xrange(start, end):
            n.node(repo.changelog.node(rev))
        n.diff(node, repo.changelog.tip())
    else:
        count = 1
        n.node(node)
        n.diff(node, node)
    data = ui.popbuffer()
    n.send(node, count, data)
