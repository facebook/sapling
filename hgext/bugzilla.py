# bugzilla.py - bugzilla integration for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# hook extension to update comments of bugzilla bugs when changesets
# that refer to bugs by id are seen.  this hook does not change bug
# status, only comments.
#
# to configure, add items to '[bugzilla]' section of hgrc.
#
# to use, configure bugzilla extension and enable like this:
#
#   [extensions]
#   hgext.bugzilla =
#
#   [hooks]
#   # run bugzilla hook on every change pulled or pushed in here
#   incoming.bugzilla = python:hgext.bugzilla.hook
#
# config items:
#
# section name is 'bugzilla'.
#  [bugzilla]
#
# REQUIRED:
#   host = bugzilla # mysql server where bugzilla database lives
#   password = **   # user's password
#   version = 2.16  # version of bugzilla installed
#
# OPTIONAL:
#   bzuser = ...    # fallback bugzilla user name to record comments with
#   db = bugs       # database to connect to
#   notify = ...    # command to run to get bugzilla to send mail
#   regexp = ...    # regexp to match bug ids (must contain one "()" group)
#   strip = 0       # number of slashes to strip for url paths
#   style = ...     # style file to use when formatting comments
#   template = ...  # template to use when formatting comments
#   timeout = 5     # database connection timeout (seconds)
#   user = bugs     # user to connect to database as
#   [web]
#   baseurl = http://hgserver/... # root of hg web site for browsing commits
#
# if hg committer names are not same as bugzilla user names, use
# "usermap" feature to map from committer email to bugzilla user name.
# usermap can be in hgrc or separate config file.
#
#   [bugzilla]
#   usermap = filename # cfg file with "committer"="bugzilla user" info
#   [usermap]
#   committer_email = bugzilla_user_name

from mercurial.i18n import _
from mercurial.node import short
from mercurial import cmdutil, templater, util
import os, re, time

MySQLdb = None

def buglist(ids):
    return '(' + ','.join(map(str, ids)) + ')'

class bugzilla_2_16(object):
    '''support for bugzilla version 2.16.'''

    def __init__(self, ui):
        self.ui = ui
        host = self.ui.config('bugzilla', 'host', 'localhost')
        user = self.ui.config('bugzilla', 'user', 'bugs')
        passwd = self.ui.config('bugzilla', 'password')
        db = self.ui.config('bugzilla', 'db', 'bugs')
        timeout = int(self.ui.config('bugzilla', 'timeout', 5))
        usermap = self.ui.config('bugzilla', 'usermap')
        if usermap:
            self.ui.readsections(usermap, 'usermap')
        self.ui.note(_('connecting to %s:%s as %s, password %s\n') %
                     (host, db, user, '*' * len(passwd)))
        self.conn = MySQLdb.connect(host=host, user=user, passwd=passwd,
                                    db=db, connect_timeout=timeout)
        self.cursor = self.conn.cursor()
        self.run('select fieldid from fielddefs where name = "longdesc"')
        ids = self.cursor.fetchall()
        if len(ids) != 1:
            raise util.Abort(_('unknown database schema'))
        self.longdesc_id = ids[0][0]
        self.user_ids = {}

    def run(self, *args, **kwargs):
        '''run a query.'''
        self.ui.note(_('query: %s %s\n') % (args, kwargs))
        try:
            self.cursor.execute(*args, **kwargs)
        except MySQLdb.MySQLError, err:
            self.ui.note(_('failed query: %s %s\n') % (args, kwargs))
            raise

    def filter_real_bug_ids(self, ids):
        '''filter not-existing bug ids from list.'''
        self.run('select bug_id from bugs where bug_id in %s' % buglist(ids))
        ids = [c[0] for c in self.cursor.fetchall()]
        ids.sort()
        return ids

    def filter_unknown_bug_ids(self, node, ids):
        '''filter bug ids from list that already refer to this changeset.'''

        self.run('''select bug_id from longdescs where
                    bug_id in %s and thetext like "%%%s%%"''' %
                 (buglist(ids), short(node)))
        unknown = dict.fromkeys(ids)
        for (id,) in self.cursor.fetchall():
            self.ui.status(_('bug %d already knows about changeset %s\n') %
                           (id, short(node)))
            unknown.pop(id, None)
        ids = unknown.keys()
        ids.sort()
        return ids

    def notify(self, ids):
        '''tell bugzilla to send mail.'''

        self.ui.status(_('telling bugzilla to send mail:\n'))
        for id in ids:
            self.ui.status(_('  bug %s\n') % id)
            cmd = self.ui.config('bugzilla', 'notify',
                               'cd /var/www/html/bugzilla && '
                               './processmail %s nobody@nowhere.com') % id
            fp = os.popen('(%s) 2>&1' % cmd)
            out = fp.read()
            ret = fp.close()
            if ret:
                self.ui.warn(out)
                raise util.Abort(_('bugzilla notify command %s') %
                                 util.explain_exit(ret)[0])
        self.ui.status(_('done\n'))

    def get_user_id(self, user):
        '''look up numeric bugzilla user id.'''
        try:
            return self.user_ids[user]
        except KeyError:
            try:
                userid = int(user)
            except ValueError:
                self.ui.note(_('looking up user %s\n') % user)
                self.run('''select userid from profiles
                            where login_name like %s''', user)
                all = self.cursor.fetchall()
                if len(all) != 1:
                    raise KeyError(user)
                userid = int(all[0][0])
            self.user_ids[user] = userid
            return userid

    def map_committer(self, user):
        '''map name of committer to bugzilla user name.'''
        for committer, bzuser in self.ui.configitems('usermap'):
            if committer.lower() == user.lower():
                return bzuser
        return user

    def add_comment(self, bugid, text, committer):
        '''add comment to bug. try adding comment as committer of
        changeset, otherwise as default bugzilla user.'''
        user = self.map_committer(committer)
        try:
            userid = self.get_user_id(user)
        except KeyError:
            try:
                defaultuser = self.ui.config('bugzilla', 'bzuser')
                if not defaultuser:
                    raise util.Abort(_('cannot find bugzilla user id for %s') %
                                     user)
                userid = self.get_user_id(defaultuser)
            except KeyError:
                raise util.Abort(_('cannot find bugzilla user id for %s or %s') %
                                 (user, defaultuser))
        now = time.strftime('%Y-%m-%d %H:%M:%S')
        self.run('''insert into longdescs
                    (bug_id, who, bug_when, thetext)
                    values (%s, %s, %s, %s)''',
                 (bugid, userid, now, text))
        self.run('''insert into bugs_activity (bug_id, who, bug_when, fieldid)
                    values (%s, %s, %s, %s)''',
                 (bugid, userid, now, self.longdesc_id))

class bugzilla(object):
    # supported versions of bugzilla. different versions have
    # different schemas.
    _versions = {
        '2.16': bugzilla_2_16,
        }

    _default_bug_re = (r'bugs?\s*,?\s*(?:#|nos?\.?|num(?:ber)?s?)?\s*'
                       r'((?:\d+\s*(?:,?\s*(?:and)?)?\s*)+)')

    _bz = None

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo

    def bz(self):
        '''return object that knows how to talk to bugzilla version in
        use.'''

        if bugzilla._bz is None:
            bzversion = self.ui.config('bugzilla', 'version')
            try:
                bzclass = bugzilla._versions[bzversion]
            except KeyError:
                raise util.Abort(_('bugzilla version %s not supported') %
                                 bzversion)
            bugzilla._bz = bzclass(self.ui)
        return bugzilla._bz

    def __getattr__(self, key):
        return getattr(self.bz(), key)

    _bug_re = None
    _split_re = None

    def find_bug_ids(self, ctx):
        '''find valid bug ids that are referred to in changeset
        comments and that do not already have references to this
        changeset.'''

        if bugzilla._bug_re is None:
            bugzilla._bug_re = re.compile(
                self.ui.config('bugzilla', 'regexp', bugzilla._default_bug_re),
                re.IGNORECASE)
            bugzilla._split_re = re.compile(r'\D+')
        start = 0
        ids = {}
        while True:
            m = bugzilla._bug_re.search(ctx.description(), start)
            if not m:
                break
            start = m.end()
            for id in bugzilla._split_re.split(m.group(1)):
                if not id: continue
                ids[int(id)] = 1
        ids = ids.keys()
        if ids:
            ids = self.filter_real_bug_ids(ids)
        if ids:
            ids = self.filter_unknown_bug_ids(ctx.node(), ids)
        return ids

    def update(self, bugid, ctx):
        '''update bugzilla bug with reference to changeset.'''

        def webroot(root):
            '''strip leading prefix of repo root and turn into
            url-safe path.'''
            count = int(self.ui.config('bugzilla', 'strip', 0))
            root = util.pconvert(root)
            while count > 0:
                c = root.find('/')
                if c == -1:
                    break
                root = root[c+1:]
                count -= 1
            return root

        mapfile = self.ui.config('bugzilla', 'style')
        tmpl = self.ui.config('bugzilla', 'template')
        t = cmdutil.changeset_templater(self.ui, self.repo,
                                        False, mapfile, False)
        if not mapfile and not tmpl:
            tmpl = _('changeset {node|short} in repo {root} refers '
                     'to bug {bug}.\ndetails:\n\t{desc|tabindent}')
        if tmpl:
            tmpl = templater.parsestring(tmpl, quoted=False)
            t.use_template(tmpl)
        self.ui.pushbuffer()
        t.show(changenode=ctx.node(), changes=ctx.changeset(),
               bug=str(bugid),
               hgweb=self.ui.config('web', 'baseurl'),
               root=self.repo.root,
               webroot=webroot(self.repo.root))
        data = self.ui.popbuffer()
        self.add_comment(bugid, data, util.email(ctx.user()))

def hook(ui, repo, hooktype, node=None, **kwargs):
    '''add comment to bugzilla for each changeset that refers to a
    bugzilla bug id. only add a comment once per bug, so same change
    seen multiple times does not fill bug with duplicate data.'''
    try:
        import MySQLdb as mysql
        global MySQLdb
        MySQLdb = mysql
    except ImportError, err:
        raise util.Abort(_('python mysql support not available: %s') % err)

    if node is None:
        raise util.Abort(_('hook type %s does not pass a changeset id') %
                         hooktype)
    try:
        bz = bugzilla(ui, repo)
        ctx = repo.changectx(node)
        ids = bz.find_bug_ids(ctx)
        if ids:
            for id in ids:
                bz.update(id, ctx)
            bz.notify(ids)
    except MySQLdb.MySQLError, err:
        raise util.Abort(_('database error: %s') % err[1])

