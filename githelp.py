# githelp.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""try mapping git commands to Mercurial commands

Tries to map a given git command to a Mercurial command:

  $ hg githelp -- git checkout master
  hg update master

"""
from mercurial import extensions, util, cmdutil, commands, error
from mercurial import hg, changegroup, fancyopts
from mercurial.extensions import wrapfunction
from hgext import pager
from mercurial.node import hex, nullrev, nullid
from mercurial.i18n import _
import errno, os, re, glob, getopt

pager.attended.append('githelp')

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

class GitUnknownError(error.Abort):
    FailMessage = """\n\nIf this is a valid git command, please log a task for the source_control oncall.\n"""
    def __init__(self, msg):
        msg = msg + GitUnknownError.FailMessage
        super(GitUnknownError, self).__init__(msg)

def convert(s):
    if s.startswith("origin/"):
        return s[7:]
    if 'HEAD' in s:
        s = s.replace('HEAD', '.')
    return s

@command('^githelp|git', [
    ], _('hg githelp'))
def githelp(ui, repo, *args, **kwargs):
    '''suggests the Mercurial equivalent of the given git command

    Usage: hg githelp -- <git command>
    '''

    if len(args) == 0:
        raise util.Abort(_('missing git command - usage: hg githelp -- <git command>'))

    if args[0] == 'git':
        args = args[1:]

    cmd = args[0]
    if not cmd in gitcommands:
        raise GitUnknownError("error: unknown git command %s" % (cmd))

    args = args[1:]
    return gitcommands[cmd](ui, repo, *args, **kwargs)

def parseoptions(ui, cmdoptions, args):
    cmdoptions = list(cmdoptions)
    opts = {}
    args = list(args)
    while True:
        try:
            args = fancyopts.fancyopts(list(args), cmdoptions, opts, True)
            break
        except getopt.GetoptError, ex:
            flag = None
            if "requires argument" in ex.msg:
                raise
            if ('--' + ex.opt) in ex.msg:
                flag = '--' + ex.opt
            elif ('-' + ex.opt) in ex.msg:
                flag = '-' + ex.opt
            else:
                raise GitUnknownError("unknown option %s" % ex.opt)
            args.remove(flag)
            ui.warn("ignoring unknown option %s\n" % flag)

    args = list([convert(x) for x in args])
    opts = dict([(k, convert(v)) if isinstance(v, str) else (k, v) for k,v in opts.iteritems()])

    return args, opts

class Command(object):
    def __init__(self, name):
        self.name = name
        self.args = []
        self.opts = {}

    def __str__(self):
        cmd = "hg " + self.name
        if self.opts:
            for k, values in self.opts.iteritems():
                for v in values:
                    if v:
                        cmd += " %s %s" % (k, v)
                    else:
                        cmd += " %s" % (k, )
        if self.args:
            cmd += " "
            cmd += " ".join(self.args)
        return cmd

    def append(self, value):
        self.args.append(value)

    def extend(self, values):
        self.args.extend(values)

    def __setitem__(self, key, value):
        values = self.opts.setdefault(key, [])
        values.append(value)

    def __and__(self, other):
        return AndCommand(self, other)

class AndCommand(object):
    def __init__(self, left, right):
        self.left = left
        self.right = right

    def __str__(self):
        return "%s && %s" % (self.left, self.right)

    def __and__(self, other):
        return AndCommand(self, other)

def add(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('A', 'all', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("add")

    if not opts.get('all'):
        cmd.extend(args)
    else:
        ui.status("note: use hg addremove to remove files that have been deleted.\n\n")

    ui.status(cmd, "\n")

def bisect(ui, repo, *args, **kwargs):
    ui.status("See 'hg help bisect' for how to use bisect.\n\n")

def blame(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)
    cmd = Command('annotate')
    cmd.extend([convert(v) for v in args])
    ui.status(cmd, "\n")

def branch(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'set-upstream', None, ''),
        ('', 'set-upstream-to', '', ''),
        ('d', 'delete', None, ''),
        ('D', 'delete', None, ''),
        ('m', 'move', None, ''),
        ('M', 'move', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("bookmark")

    if opts.get('set_upstream') or opts.get('set_upstream_to'):
        ui.status("Mercurial has no concept of upstream branches\n")
        return
    elif opts.get('delete'):
        cmd['-d'] = None
        cmd.append(args[0])
    elif opts.get('move'):
        cmd['-m'] = args[0]
        if len(args) > 1:
            cmd.append(args[1])
    else:
        if len(args) > 1:
            cmd['-r'] = args[1]
            cmd.append(args[0])
        elif len(args) == 1:
            cmd.append(args[0])

    ui.status(cmd, "\n")

def checkout(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('b', 'branch', '', ''),
        ('B', 'branch', '', ''),
        ('f', 'force', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('update')

    if opts.get('force'):
        cmd['-C'] = None

    if opts.get('branch'):
        if len(args) == 0:
            cmd = Command('bookmark')
            cmd.append(opts.get('branch'))
        else:
            cmd.append(args[0])
            bookcmd = Command('bookmark')
            bookcmd.append(opts.get('branch'))
            cmd = cmd & bookcmd
    elif len(args) == 0:
        raise GitUnknownError("a commit must be specified")
    elif len(args) > 1:
        cmd = Command('revert')
        cmd['-r'] = args[0]
        cmd.extend(args[1:])
    else:
        cmd.append(args[0])

    ui.status(cmd, "\n")

def cherrypick(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'continue', None, ''),
        ('', 'abort', None, ''),
        ('e', 'edit', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('graft')

    if opts.get('edit'):
        cmd['--edit'] = None
    if opts.get('continue'):
        cmd['--continue'] = None
    elif opts.get('abort'):
        ui.status("note: hg graft does not have --abort. I don't know why.\n\n")
        return
    else:
        cmd.extend(args)

    ui.status(cmd, "\n")

def clean(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('d', 'd', None, ''),
        ('f', 'force', None, ''),
        ('x', 'x', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('purge')
    if opts.get('x'):
        cmd['--all'] = None
    cmd.extend(args)

    ui.status(cmd, "\n")

def clone(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'bare', None, ''),
        ('n', 'no-checkout', None, ''),
        ('b', 'branch', '', ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('clone')
    cmd.append(args[0])
    if len(args) > 1:
        cmd.append(args[1])

    if opts.get('bare'):
        cmd['-U'] = None
        ui.status("note: Mercurial does not have bare clones. " +
            "-U will clone the repo without checking out a commit\n\n")
    elif opts.get('no_checkout'):
        cmd['-U'] = None

    if opts.get('branch'):
        cocmd = Command("update")
        cocmd.append(opts.get('branch'))
        cmd = cmd & cocmd

    ui.status(cmd, "\n")

def commit(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('a', 'all', None, ''),
        ('m', 'message', '', ''),
        ('p', 'patch', None, ''),
        ('F', 'file', '', ''),
        ('', 'author', '', ''),
        ('', 'date', '', ''),
        ('', 'amend', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('commit')
    if opts.get('patch'):
        cmd = Command('record')

    if opts.get('message'):
        cmd['-m'] = "'%s'" % (opts.get('message'),)

    if opts.get('all'):
        ui.status("note: Mercurial doesn't have a staging area, " +
            "so there is no --all. -A will add and remove files " +
            "for you though.\n\n")

    if opts.get('file'):
        cmd['-l'] = opts.get('file')

    if opts.get('author'):
        cmd['-u'] = opts.get('author')

    if opts.get('date'):
        cmd['-d'] = opts.get('date')

    if opts.get('amend'):
        cmd['--amend'] = None

    cmd.extend(args)

    ui.status(cmd, "\n")

def diff(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('a', 'all', None, ''),
        ('', 'cached', None, ''),
        ('R', 'reverse', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('diff')

    if opts.get('cached'):
        ui.status('note: Mercurial has no concept of a staging area, ' +
            'so --cached does nothing.\n\n')

    if opts.get('reverse'):
        cmd['--reverse'] = None

    for a in list(args):
        args.remove(a)
        try:
            repo.revs(a)
            cmd['-r'] = a
        except:
            cmd.append(a)

    ui.status(cmd, "\n")

def fetch(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'all', None, ''),
        ('f', 'force', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('pull')

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status("note: Mercurial doesn't have refspecs. " +
                "-r can be used to specify which commits you want to pull. " +
                "-B can be used to specify which bookmark you want to pull.\n\n")
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd['-B'] = v
                else:
                    cmd['-r'] = v

    ui.status(cmd, "\n")

def grep(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('grep')

    if len(args) > 0:
        cmd.append(args[0])

    ui.status(cmd, "\n")

def init(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('init')

    if len(args) > 0:
        cmd.append(args[0])

    ui.status(cmd, "\n")

def log(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'follow', None, ''),
        ('', 'decorate', None, ''),
        ('n', 'number', '', ''),
        ('1', '1', None, ''),
        ('', 'pretty', '', ''),
        ('', 'format', '', ''),
        ('', 'oneline', None, ''),
        ('', 'stat', None, ''),
        ('', 'graph', None, ''),
        ('p', 'patch', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    ui.status("note: see hg help revset for information on how to filter " +
        "log output.\n\n")

    cmd = Command('log')
    cmd['-f'] = None

    if opts.get('number'):
        cmd['-l'] = opts.get('number')
    if opts.get('1'):
        cmd['-l'] = '1'
    if opts.get('stat'):
        cmd['--stat'] = None
    if opts.get('graph'):
        cmd['-G'] = None
    if opts.get('patch'):
        cmd['-p'] = None

    if opts.get('pretty') or opts.get('format') or opts.get('oneline'):
        format = opts.get('format', '')
        if 'format:' in format:
            ui.status("note: --format format:??? equates to Mercurial's " +
                "--template. See hg help templates for more info.\n\n")
            cmd['--template'] = '???'
        else:
            ui.status("note: --pretty/format/oneline equate to Mercurial's " +
                "--style or --template. See hg help templates for more info.\n\n")
            cmd['--style'] = '???'

    if len(args) > 0:
        if '..' in args[0]:
            since, until = args[0].split('..')
            cmd['-r'] = "'%s::%s'" % (since, until)
            del args[0]
        cmd.extend(args)

    ui.status(cmd, "\n")

def lsfiles(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('c', 'cached', None, ''),
        ('d', 'deleted', None, ''),
        ('m', 'modified', None, ''),
        ('o', 'others', None, ''),
        ('i', 'ignored', None, ''),
        ('s', 'stage', None, ''),
        ('z', '_zero', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if (opts.get('modified') or opts.get('deleted')
        or opts.get('others') or opts.get('ignored')):
        cmd = Command('status')
        if opts.get('deleted'):
            cmd['-d'] = None
        if opts.get('modified'):
            cmd['-m'] = None
        if opts.get('others'):
            cmd['-o'] = None
        if opts.get('ignored'):
            cmd['-i'] = None
    else:
        cmd = Command('files')
    if opts.get('stage'):
        ui.status("note: Mercurial doesn't have a staging area, ignoring "
                  "--stage\n")
    if opts.get('_zero'):
        cmd['-0'] = None
    cmd.append('.')
    for include in args:
        cmd['-I'] = util.shellquote(include)

    ui.status(cmd, "\n")

def merge(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('merge')

    if len(args) > 0:
        cmd.append(args[len(args) - 1])

    ui.status(cmd, "\n")

def mv(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('f', 'force', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('mv')
    cmd.extend(args)

    if opts.get('force'):
        cmd['-f'] = None

    ui.status(cmd, "\n")

def pull(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'all', None, ''),
        ('f', 'force', None, ''),
        ('r', 'rebase', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('pull')
    cmd['--rebase'] = None

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status("note: Mercurial doesn't have refspecs. " +
                "-r can be used to specify which commits you want to pull. " +
                "-B can be used to specify which bookmark you want to pull.\n\n")
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd['-B'] = v
                else:
                    cmd['-r'] = v

    ui.status(cmd, "\n")

def push(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'all', None, ''),
        ('f', 'force', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('push')

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status("note: Mercurial doesn't have refspecs. " +
                "-r can be used to specify which commits you want to push. " +
                "-B can be used to specify which bookmark you want to push.\n\n")
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd['-B'] = v
                else:
                    cmd['-r'] = v

    if opts.get('force'):
        cmd['-f'] = None

    ui.status(cmd, "\n")

def rebase(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'all', None, ''),
        ('i', 'interactive', None, ''),
        ('', 'onto', '', ''),
        ('', 'abort', None, ''),
        ('', 'continue', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get('interactive'):
        ui.status("note: hg histedit does not perform a rebase. " +
            "It just edits history.\n\n")
        ui.status("also note: the complicated first(children(blahblah)) below " +
            "is just a direct translation from git. It's much simpler to just specify " +
            "the lowest commit that should be part of the histedit. Example: if " +
            "you have commits (0 <- 1 <- 2 <- 3) and you want to edit 2+3 " +
            "(assuming you are on 3 right now) use 'hg histedit 2'.\n\n")
        cmd = Command('histedit')
        cmd.append("'first(children(ancestor(.,%s))::.)'" % (convert(args[0]),))
        ui.status(cmd, "\n")
        return

    cmd = Command('rebase')

    if opts.get('continue'):
        cmd['--continue'] = None
    if opts.get('abort'):
        cmd['--abort'] = None

    if opts.get('onto'):
        ui.status("note: if you're trying to lift a commit off one branch, " +
            "try using hg rebase -d <destination commit> -s <commit to be lifted>\n\n")
        cmd['-d'] = convert(opts.get('onto'))
        if len(args) < 2:
            raise GitUnknownError("Expected format: git rebase --onto X Y Z")
        cmd['-s'] = "'::%s - ::%s'" % (convert(args[1]), convert(args[0]))
    else:
        if len(args) == 1:
            cmd['-d'] = convert(args[0])
        elif len(args) == 2:
            cmd['-d'] = convert(args[0])
            cmd['-b'] = convert(args[1])

    ui.status(cmd, "\n")

def reset(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'soft', None, ''),
        ('', 'hard', None, ''),
        ('', 'mixed', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    commit = convert(args[0] if len(args) > 0 else '.')
    hard = opts.get('hard')

    try:
        revs = repo.revs(commit)
        parentreset = revs and revs[0] == repo.revs('.^')[0]
        selfreset = revs and revs[0] == repo.revs('.')[0]
    except:
        parentreset = False
        selfreset = False

    # Case 1: undo a commit
    if parentreset:
        if hard:
            ui.status("note: hg strip will delete the commit entirely.\n\n")

            cmd = Command('strip')
            cmd['-r'] = '.'
        else:
            ui.status("note: hg strip -k will delete the commit, but keep the " +
                "changes in your working copy.\n\n")
            cmd = Command('strip')
            cmd['-k'] = None
            cmd['-r'] = '.'
    # Case 2: clearing pending changes
    elif hard and selfreset:
        cmd = Command('revert')
        cmd['--all'] = None
    # Case 3: move a bookmark
    else:
        upcmd = Command('update')
        upcmd.append(commit)
        bookcmd = Command('bookmark')
        bookcmd['-f'] = None
        bookcmd.append('<bookmarkname>')
        cmd = upcmd & bookcmd

    ui.status(cmd, "\n")

def revert(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if len(args) > 1:
        ui.status("note: hg backout doesn't support multiple commits at once\n\n")

    cmd = Command('backout')
    cmd.append(args[0])

    ui.status(cmd, "\n")

def rm(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('f', 'force', None, ''),
        ('n', 'dry-run', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('rm')
    cmd.extend(args)

    if opts.get('force'):
        cmd['-f'] = None
    if opts.get('dry_run'):
        cmd['-n'] = None

    ui.status(cmd, "\n")

def show(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('export')
    cmd.extend([convert(v) for v in args])

    ui.status(cmd, "\n")

def stash(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('shelve')
    action = args[0] if len(args) > 0 else None

    if action == 'list':
        cmd['-l'] = None
    elif action == 'drop':
        cmd['-d'] = None
        if len(args) > 1:
            cmd['--name'] = args[1]
        else:
            cmd['--name'] = '<shelve name>'
    elif action == 'pop' or action == 'apply':
        cmd = Command('unshelve')
        if len(args) > 1:
            cmd.append(args[1])
        if action == 'apply':
            cmd['--keep'] = None
    elif (action == 'branch' or action == 'show' or action == 'clear'
        or action == 'create'):
        ui.status("note: Mercurial doesn't have equivalents to the " +
            "git stash branch, show, clear, or create actions.\n\n")
        return
    else:
        if len(args) > 0:
            if args[0] != 'save':
                cmd['--name'] = args[0]
            elif len(args) > 1:
                cmd['--name'] = args[1]

    ui.status(cmd, "\n")

def status(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('', 'ignored', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('status')
    cmd.extend(args)

    if opts.get('ignored'):
        cmd['-i'] = None

    ui.status(cmd, "\n")

def svn(ui, repo, *args, **kwargs):
    svncmd = args[0]
    if not svncmd in gitsvncommands:
        ui.warn("error: unknown git svn command %s\n" % (svncmd))

    args = args[1:]
    return gitsvncommands[svncmd](ui, repo, *args, **kwargs)

def svndcommit(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('push')

    ui.status(cmd, "\n")

def svnfetch(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('pull')
    cmd.append('default-push')

    ui.status(cmd, "\n")

def svnfindrev(ui, repo, *args, **kwargs):
    cmdoptions = [
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command('log')
    cmd['-r'] = args[0]

    ui.status(cmd, "\n")

def svnrebase(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('l', 'local', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    pullcmd = Command('pull')
    pullcmd.append('default-push')
    rebasecmd = Command('rebase')
    rebasecmd.append('tip')

    cmd = pullcmd & rebasecmd

    ui.status(cmd, "\n")

def tag(ui, repo, *args, **kwargs):
    cmdoptions = [
        ('f', 'force', None, ''),
        ('l', 'list', None, ''),
        ('d', 'delete', None, ''),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get('list'):
        cmd = Command('tags')
    else:
        cmd = Command('tag')
        cmd.append(args[0])
        if len(args) > 1:
            cmd['-r'] = args[1]

        if opts.get('delete'):
            cmd['--remove'] = None

        if opts.get('force'):
            cmd['-f'] = None

    ui.status(cmd, "\n")

gitcommands = {
    'add': add,
    'bisect': bisect,
    'blame': blame,
    'branch': branch,
    'checkout': checkout,
    'cherry-pick': cherrypick,
    'clean': clean,
    'clone': clone,
    'commit': commit,
    'diff': diff,
    'fetch': fetch,
    'grep': grep,
    'init': init,
    'log': log,
    'ls-files': lsfiles,
    'merge': merge,
    'mv': mv,
    'pull': pull,
    'push': push,
    'rebase': rebase,
    'reset': reset,
    'revert': revert,
    'rm': rm,
    'show': show,
    'stash': stash,
    'status': status,
    'svn': svn,
    'tag': tag,
}

gitsvncommands = {
    'dcommit': svndcommit,
    'fetch': svnfetch,
    'find-rev': svnfindrev,
    'rebase': svnrebase,
}
