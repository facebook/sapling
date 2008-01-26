# mail.py - mail sending bits for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import os, smtplib, templater, util, socket

def _smtp(ui):
    '''send mail using smtp.'''

    local_hostname = ui.config('smtp', 'local_hostname')
    s = smtplib.SMTP(local_hostname=local_hostname)
    mailhost = ui.config('smtp', 'host')
    if not mailhost:
        raise util.Abort(_('no [smtp]host in hgrc - cannot send mail'))
    mailport = int(ui.config('smtp', 'port', 25))
    ui.note(_('sending mail: smtp host %s, port %s\n') %
            (mailhost, mailport))
    s.connect(host=mailhost, port=mailport)
    if ui.configbool('smtp', 'tls'):
        if not hasattr(socket, 'ssl'):
            raise util.Abort(_("can't use TLS: Python SSL support "
                               "not installed"))
        ui.note(_('(using tls)\n'))
        s.ehlo()
        s.starttls()
        s.ehlo()
    username = ui.config('smtp', 'username')
    password = ui.config('smtp', 'password')
    if username and not password:
        password = ui.getpass()
    if username and password:
        ui.note(_('(authenticating to mail server as %s)\n') %
                  (username))
        s.login(username, password)
    return s

class _sendmail(object):
    '''send mail using sendmail.'''

    def __init__(self, ui, program):
        self.ui = ui
        self.program = program

    def sendmail(self, sender, recipients, msg):
        cmdline = '%s -f %s %s' % (
            self.program, templater.email(sender),
            ' '.join(map(templater.email, recipients)))
        self.ui.note(_('sending mail: %s\n') % cmdline)
        fp = os.popen(cmdline, 'w')
        fp.write(msg)
        ret = fp.close()
        if ret:
            raise util.Abort('%s %s' % (
                os.path.basename(self.program.split(None, 1)[0]),
                util.explain_exit(ret)[0]))

def connect(ui):
    '''make a mail connection. object returned has one method, sendmail.
    call as sendmail(sender, list-of-recipients, msg).'''

    method = ui.config('email', 'method', 'smtp')
    if method == 'smtp':
        return _smtp(ui)

    return _sendmail(ui, method)

def sendmail(ui, sender, recipients, msg):
    try:
        return connect(ui).sendmail(sender, recipients, msg)
    except smtplib.SMTPRecipientsRefused, inst:
        recipients = [r[1] for r in inst.recipients.values()]
        raise util.Abort('\n' + '\n'.join(recipients))
    except smtplib.SMTPException, inst:
        raise util.Abort(inst)

def validateconfig(ui):
    '''determine if we have enough config data to try sending email.'''
    method = ui.config('email', 'method', 'smtp')
    if method == 'smtp':
        if not ui.config('smtp', 'host'):
            raise util.Abort(_('smtp specified as email transport, '
                               'but no smtp host configured'))
    else:
        if not util.find_exe(method):
            raise util.Abort(_('%r specified as email transport, '
                               'but not in PATH') % method)
