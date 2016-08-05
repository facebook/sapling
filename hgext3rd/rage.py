# Copyright 2014 Facebook Inc.
#
"""upload useful diagnostics and give instructions for asking for help"""

from mercurial.i18n import _
from mercurial import cmdutil, util, commands, bookmarks, ui, extensions, error
from mercurial import scmutil
from hgext import blackbox
from hgext3rd import (
    smartlog,
    sparse,
)
import os, socket, re, time, traceback

cmdtable = {}
command = cmdutil.command(cmdtable)

_failsafeerrors = []

def _failsafe(func):
    try:
        return func()
    except Exception as ex:
        index = len(_failsafeerrors) + 1
        message = "[%d]: %s\n%s\n" % (index, str(ex), traceback.format_exc())
        _failsafeerrors.append(message)
        return '(Failed. See footnote [%d])' % index

def shcmd(cmd, input=None, check=True, keeperr=True):
    _, _, _, p = util.popen4(cmd)
    out, err = p.communicate(input)
    if check and p.returncode:
        raise error.Abort(cmd + ' error: ' + err)
    elif keeperr:
        out += err
    return out

def createtask(ui, repo, defaultdesc):
    """FBONLY: create task for source control oncall"""
    prompt = '''Title: [hg rage] %s on %s by %s

Description:
%s

HG: Edit task title and description. Lines beginning with 'HG:' are removed."
HG: First line is the title followed by the description.
HG: Feel free to add relevant information.
''' % (repo.root, socket.gethostname(), os.getenv('LOGNAME'), defaultdesc)

    text = re.sub("(?m)^HG:.*(\n|$)", "", ui.edit(prompt, ui.username()))
    lines = text.splitlines()
    title = re.sub("(?m)^Title:\s+", "", lines[0])
    desc = re.sub("(?m)^Description:\s+", "", '\n'.join(lines[1:]))
    tag = 'hg rage'
    oncall = 'source_control'
    taskid = shcmd(' '.join([
        'tasks',
        'create',
        '--title=' + util.shellquote(title),
        '--pri=low',
        '--assign=' + util.shellquote(oncall),
        '--sub=' + util.shellquote(oncall),
        '--tag=' + util.shellquote(tag),
        '--desc=' + util.shellquote(desc),
        ])
    )
    tasknum = shcmd('tasks view ' + taskid).splitlines()[0].split()[0]
    print('Task created: https://our.intern.facebook.com/intern/tasks/?t=%s'
          % tasknum)

def which(name):
    """ """
    for p in os.environ.get('PATH', '/bin').split(os.pathsep):
        path = os.path.join(p, name)
        if os.path.exists(path):
            return path
    return None

rageopts = [('p', 'preview', None,
             _('print diagnostic information without doing arc paste'))]
if which('oncalls'):
    rageopts.append(('', 'oncall', None,
                     _('file a task for source control oncall')))

def localconfig(ui):
    result = []
    for section, name, value in ui.walkconfig():
        source = ui.configsource(section, name)
        if source.find('/etc/') == -1 and source.find('/default.d/') == -1:
            result.append('%s.%s=%s  # %s' % (section, name, value, source))
    return result

def obsoleteinfo(repo, hgcmd):
    """Return obsolescence markers that are relevant to smartlog revset"""
    unfi = repo.unfiltered()
    revs = scmutil.revrange(unfi, ["smartlog()"])
    hashes = '|'.join(unfi[rev].hex() for rev in revs)
    markers = hgcmd(commands.debugobsolete, rev=[])
    pat = re.compile('(^.*(?:'+hashes+').*$)', re.MULTILINE)
    relevant = pat.findall(markers)
    return "\n".join(relevant)

def usechginfo():
    """FBONLY: Information about whether chg is enabled"""
    files = {
        'system': '/etc/mercurial/usechg',
        'user': os.path.expanduser('~/.usechg'),
    }
    result = []
    for name, path in files.items():
        if os.path.exists(path):
            with open(path) as f:
                value = f.read().strip()
        else:
            value = '(not set)'
        result.append('%s: %s' % (name, value))
    return '\n'.join(result)

def rpminfo():
    """FBONLY: Information about RPM packages"""
    result = set()
    for name in ['hg', 'hg.real']:
        path = which(name)
        if not path:
            continue
        result.add(shcmd('rpm -qf %s' % path, check=False))
    return ''.join(result)

@command('^rage', rageopts , _('hg rage'))
def rage(ui, repo, *pats, **opts):
    """collect useful diagnostics for asking help from the source control team

    The rage command collects useful diagnostic information.

    By default, the information will be uploaded to Phabricator and
    instructions about how to ask for help will be printed.
    """

    def format(pair, basic=True):
        if basic:
            fmt = "%s: %s\n"
        else:
            fmt =  "%s:\n---------------------------\n%s\n"
        return fmt % pair

    def hgcmd(func, *args, **opts):
        ui.pushbuffer()
        func(ui, repo, *args, **opts)
        return ui.popbuffer()

    if opts.get('oncall') and opts.get('preview'):
        raise error.Abort('--preview and --oncall cannot be used together')

    basic = [
        ('date', time.ctime()),
        ('unixname', os.getenv('LOGNAME')),
        ('hostname', socket.gethostname()),
        ('repo location', _failsafe(lambda: repo.root)),
        ('active bookmark',
            _failsafe(lambda: bookmarks._readactive(repo, repo._bookmarks))),
        ('hg version', _failsafe(
            lambda: __import__('mercurial.__version__').__version__.version)),
        ('obsstore size', _failsafe(
            lambda: str(repo.vfs.stat('store/obsstore').st_size))),
    ]

    ui._colormode = None

    detailed = [
        ('df -h', _failsafe(lambda: shcmd('df -h', check=False))),
        ('hg sl', _failsafe(lambda: hgcmd(smartlog.smartlog, all=True))),
        ('first 20 lines of "hg status"',
            _failsafe(lambda:
                '\n'.join(hgcmd(commands.status).splitlines()[:20]))),
        ('last 40 lines of interesting blackbox.log',
            _failsafe(lambda: shcmd('grep -Fv FM: %s | tail -n 40'
                                    % util.shellquote(ui._bbfp.name)))),
        ('hg summary', _failsafe(lambda: hgcmd(commands.summary))),
        ('hg config (local)', _failsafe(lambda: '\n'.join(localconfig(ui)))),
        ('hg sparse',
            _failsafe(
                lambda: hgcmd(
                    sparse.sparse, include=False, exclude=False, delete=False,
                    force=False, enable_profile=False, disable_profile=False,
                    refresh=False, reset=False))),
        ('usechg', _failsafe(usechginfo)),
        ('rpm info', _failsafe(rpminfo)),
        ('ifconfig', _failsafe(lambda: shcmd('ifconfig'))),
        ('airport', _failsafe(
            lambda: shcmd('/System/Library/PrivateFrameworks/Apple80211.' +
                          'framework/Versions/Current/Resources/airport ' +
                          '--getinfo', check=False))),
        ('hg debugobsolete <smartlog>',
            _failsafe(lambda: obsoleteinfo(repo, hgcmd))),
        ('hg config (all)', _failsafe(lambda: hgcmd(commands.config))),
    ]

    # This is quite slow, so we don't want to do it by default
    if ui.configbool("rage", "fastmanifestcached", False):
        detailed.append(
            ('hg sl -r "fastmanifestcached()"',
                _failsafe(lambda: hgcmd(smartlog.smartlog,
                          rev=["fastmanifestcached()"]))),
        )

    msg = '\n'.join(map(format, basic)) + '\n' +\
          '\n'.join(map(lambda x: format(x, False), detailed))
    if _failsafeerrors:
        msg += '\n' + '\n'.join(_failsafeerrors)

    if opts.get('preview'):
        print(msg)
        return

    pasteurl = shcmd('arc paste --lang hgrage', msg).split()[1]

    if opts.get('oncall'):
        createtask(ui, repo, 'rage info: %s' % pasteurl)
    else:
        print('Please post your problem and the following link at'
              ' %s for help:\n%s'
              % (ui.config('ui', 'supportcontact'), pasteurl))
