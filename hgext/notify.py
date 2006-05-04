from mercurial.demandload import *
from mercurial.i18n import gettext as _
from mercurial.node import *
demandload(globals(), 'email.MIMEText mercurial:templater,util fnmatch socket')
demandload(globals(), 'time')

class notifier(object):
    def __init__(self, ui, repo):
        self.ui = ui
        self.ui.readconfig(self.ui.config('notify', 'config'))
        self.repo = repo
        self.stripcount = self.ui.config('notify', 'strip')
        self.root = self.strip(self.repo.root)

    def strip(self, path):
        path = util.pconvert(path)
        count = self.stripcount
        while path and count >= 0:
            c = path.find('/')
            if c == -1:
                break
            path = path[c+1:]
            count -= 1
        return path

    def subscribers(self):
        subs = []
        for user, pat in self.ui.configitems('usersubs'):
            if fnmatch.fnmatch(self.root, pat):
                subs.append(user)
        for pat, users in self.ui.configitems('reposubs'):
            if fnmatch.fnmatch(self.root, pat):
                subs.extend([u.strip() for u in users.split(',')])
        subs.sort()
        return subs

    def seen(self, node):
        pass

    def url(self, path=None):
        return self.ui.config('web', 'baseurl') + (path or self.root)

    def message(self, node, changes):
        sio = templater.stringio()
        seen = self.seen(node)
        if seen:
            seen = self.strip(seen)
            sio.write('Changeset %s merged to %s\n' %
                      (short(node), self.url()))
            sio.write('First seen in %s\n' % self.url(seen))
        else:
            sio.write('Changeset %s new to %s\n' % (short(node), self.url()))
        sio.write('Committed by %s at %s\n' %
                  (changes[1], templater.isodate(changes[2])))
        sio.write('See %s?cmd=changeset;node=%s for full details\n' %
                  (self.url(), short(node)))
        sio.write('\nDescription:\n')
        sio.write(templater.indent(changes[4], '  '))
        msg = email.MIMEText.MIMEText(sio.getvalue(), 'plain')
        firstline = changes[4].lstrip().split('\n', 1)[0].rstrip()
        subject = '%s %s: %s' % (self.root, self.repo.rev(node), firstline)
        if seen:
            subject = '[merge] ' + subject
        if subject.endswith('.'):
            subject = subject[:-1]
        if len(subject) > 67:
            subject = subject[:64] + '...'
        msg['Subject'] = subject
        msg['X-Hg-Repo'] = self.root
        if '@' in changes[1]:
            msg['From'] = changes[1]
        else:
            msg['From'] = self.ui.config('email', 'from')
        msg['Message-Id'] = '<hg.%s.%s.%s@%s>' % (hex(node),
                                                  int(time.time()),
                                                  hash(self.repo.root),
                                                  socket.getfqdn())
        return msg

    def node(self, node):
        mail = self.ui.sendmail()
        changes = self.repo.changelog.read(node)
        fromaddr = self.ui.config('email', 'from', changes[1])
        msg = self.message(node, changes)
        subs = self.subscribers()
        msg['To'] = ', '.join(subs)
        msgtext = msg.as_string(0)
        mail.sendmail(templater.email(fromaddr),
                      [templater.email(s) for s in subs],
                      msgtext)


def hook(ui, repo, hooktype, node=None, **kwargs):
    n = notifier(ui, repo)
    n.node(bin(node))
