# ui.py - user interface bits for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import ConfigParser
from i18n import gettext as _
from demandload import *
demandload(globals(), "errno getpass os re smtplib socket sys tempfile")
demandload(globals(), "templater traceback util")

class ui(object):
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, traceback=False, parentui=None):
        self.overlay = {}
        if parentui is None:
            # this is the parent of all ui children
            self.parentui = None
            self.cdata = ConfigParser.SafeConfigParser()
            self.readconfig(util.rcpath())

            self.quiet = self.configbool("ui", "quiet")
            self.verbose = self.configbool("ui", "verbose")
            self.debugflag = self.configbool("ui", "debug")
            self.interactive = self.configbool("ui", "interactive", True)
            self.traceback = traceback

            self.updateopts(verbose, debug, quiet, interactive)
            self.diffcache = None
            self.header = []
            self.prev_header = []
            self.revlogopts = self.configrevlog()
        else:
            # parentui may point to an ui object which is already a child
            self.parentui = parentui.parentui or parentui
            parent_cdata = self.parentui.cdata
            self.cdata = ConfigParser.SafeConfigParser(parent_cdata.defaults())
            # make interpolation work
            for section in parent_cdata.sections():
                self.cdata.add_section(section)
                for name, value in parent_cdata.items(section, raw=True):
                    self.cdata.set(section, name, value)

    def __getattr__(self, key):
        return getattr(self.parentui, key)

    def updateopts(self, verbose=False, debug=False, quiet=False,
                   interactive=True, traceback=False, config=[]):
        self.quiet = (self.quiet or quiet) and not verbose and not debug
        self.verbose = (self.verbose or verbose) or debug
        self.debugflag = (self.debugflag or debug)
        self.interactive = (self.interactive and interactive)
        self.traceback = self.traceback or traceback
        for cfg in config:
            try:
                name, value = cfg.split('=', 1)
                section, name = name.split('.', 1)
                if not self.cdata.has_section(section):
                    self.cdata.add_section(section)
                if not section or not name:
                    raise IndexError
                self.cdata.set(section, name, value)
            except (IndexError, ValueError):
                raise util.Abort(_('malformed --config option: %s') % cfg)

    def readconfig(self, fn, root=None):
        if isinstance(fn, basestring):
            fn = [fn]
        for f in fn:
            try:
                self.cdata.read(f)
            except ConfigParser.ParsingError, inst:
                raise util.Abort(_("Failed to parse %s\n%s") % (f, inst))
        # translate paths relative to root (or home) into absolute paths
        if root is None:
            root = os.path.expanduser('~')
        for name, path in self.configitems("paths"):
            if path and path.find("://") == -1 and not os.path.isabs(path):
                self.cdata.set("paths", name, os.path.join(root, path))

    def setconfig(self, section, name, val):
        self.overlay[(section, name)] = val

    def config(self, section, name, default=None):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            try:
                return self.cdata.get(section, name)
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
        if self.parentui is None:
            return default
        else:
            return self.parentui.config(section, name, default)

    def configbool(self, section, name, default=False):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            try:
                return self.cdata.getboolean(section, name)
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
        if self.parentui is None:
            return default
        else:
            return self.parentui.configbool(section, name, default)

    def has_config(self, section):
        '''tell whether section exists in config.'''
        return self.cdata.has_section(section)

    def configitems(self, section):
        items = {}
        if self.parentui is not None:
            items = dict(self.parentui.configitems(section))
        if self.cdata.has_section(section):
            try:
                items.update(dict(self.cdata.items(section)))
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
        x = items.items()
        x.sort()
        return x

    def walkconfig(self, seen=None):
        if seen is None:
            seen = {}
        for (section, name), value in self.overlay.iteritems():
            yield section, name, value
            seen[section, name] = 1
        for section in self.cdata.sections():
            for name, value in self.cdata.items(section):
                if (section, name) in seen: continue
                yield section, name, value.replace('\n', '\\n')
                seen[section, name] = 1
        if self.parentui is not None:
            for parent in self.parentui.walkconfig(seen):
                yield parent

    def extensions(self):
        ret = self.configitems("extensions")
        for i, (k, v) in enumerate(ret):
            if v: ret[i] = (k, os.path.expanduser(v))
        return ret

    def hgignorefiles(self):
        result = []
        cfgitems = self.configitems("ui")
        for key, value in cfgitems:
            if key == 'ignore' or key.startswith('ignore.'):
                path = os.path.expanduser(value)
                result.append(path)
        return result

    def configrevlog(self):
        ret = {}
        for x in self.configitems("revlog"):
            k = x[0].lower()
            ret[k] = x[1]
        return ret
    def diffopts(self):
        if self.diffcache:
            return self.diffcache
        ret = { 'showfunc' : True, 'ignorews' : False}
        for x in self.configitems("diff"):
            k = x[0].lower()
            v = x[1]
            if v:
                v = v.lower()
                if v == 'true':
                    value = True
                else:
                    value = False
                ret[k] = value
        self.diffcache = ret
        return ret

    def username(self):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        Abort if found username is an empty string to force specifying
        the commit user elsewhere, e.g. with line option or repo hgrc.
        If not found, use ($LOGNAME or $USER or $LNAME or
        $USERNAME) +"@full.hostname".
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
        if user is None:
            user = os.environ.get("EMAIL")
        if user is None:
            try:
                user = '%s@%s' % (getpass.getuser(), socket.getfqdn())
            except KeyError:
                raise util.Abort(_("Please specify a username."))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose: user = util.shortuser(user)
        return user

    def expandpath(self, loc):
        """Return repository location relative to cwd or from [paths]"""
        if loc.find("://") != -1 or os.path.exists(loc):
            return loc

        return self.config("paths", loc, loc)

    def write(self, *args):
        if self.header:
            if self.header != self.prev_header:
                self.prev_header = self.header
                self.write(*self.header)
            self.header = []
        for a in args:
            sys.stdout.write(str(a))

    def write_header(self, *args):
        for a in args:
            self.header.append(str(a))

    def write_err(self, *args):
        try:
            if not sys.stdout.closed: sys.stdout.flush()
            for a in args:
                sys.stderr.write(str(a))
        except IOError, inst:
            if inst.errno != errno.EPIPE:
                raise

    def flush(self):
        try: sys.stdout.flush()
        except: pass
        try: sys.stderr.flush()
        except: pass

    def readline(self):
        return sys.stdin.readline()[:-1]
    def prompt(self, msg, pat=None, default="y"):
        if not self.interactive: return default
        while 1:
            self.write(msg, " ")
            r = self.readline()
            if not pat or re.match(pat, r):
                return r
            else:
                self.write(_("unrecognized response\n"))
    def getpass(self, prompt=None, default=None):
        if not self.interactive: return default
        return getpass.getpass(prompt or _('password: '))
    def status(self, *msg):
        if not self.quiet: self.write(*msg)
    def warn(self, *msg):
        self.write_err(*msg)
    def note(self, *msg):
        if self.verbose: self.write(*msg)
    def debug(self, *msg):
        if self.debugflag: self.write(*msg)
    def edit(self, text, user):
        (fd, name) = tempfile.mkstemp(prefix="hg-editor-", suffix=".txt",
                                      text=True)
        try:
            f = os.fdopen(fd, "w")
            f.write(text)
            f.close()

            editor = (os.environ.get("HGEDITOR") or
                    self.config("ui", "editor") or
                    os.environ.get("EDITOR", "vi"))

            util.system("%s \"%s\"" % (editor, name),
                        environ={'HGUSER': user},
                        onerr=util.Abort, errprefix=_("edit failed"))

            f = open(name)
            t = f.read()
            f.close()
            t = re.sub("(?m)^HG:.*\n", "", t)
        finally:
            os.unlink(name)

        return t

    def sendmail(self):
        '''send mail message. object returned has one method, sendmail.
        call as sendmail(sender, list-of-recipients, msg).'''

        def smtp():
            '''send mail using smtp.'''

            s = smtplib.SMTP()
            mailhost = self.config('smtp', 'host')
            if not mailhost:
                raise util.Abort(_('no [smtp]host in hgrc - cannot send mail'))
            mailport = int(self.config('smtp', 'port', 25))
            self.note(_('sending mail: smtp host %s, port %s\n') %
                      (mailhost, mailport))
            s.connect(host=mailhost, port=mailport)
            if self.configbool('smtp', 'tls'):
                self.note(_('(using tls)\n'))
                s.ehlo()
                s.starttls()
                s.ehlo()
            username = self.config('smtp', 'username')
            password = self.config('smtp', 'password')
            if username and password:
                self.note(_('(authenticating to mail server as %s)\n') %
                          (username))
                s.login(username, password)
            return s

        class sendmail(object):
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

        method = self.config('email', 'method', 'smtp')
        if method == 'smtp':
            mail = smtp()
        else:
            mail = sendmail(self, method)
        return mail

    def print_exc(self):
        '''print exception traceback if traceback printing enabled.
        only to call in exception handler. returns true if traceback
        printed.'''
        if self.traceback:
            traceback.print_exc()
        return self.traceback
