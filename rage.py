# Copyright 2014 Facebook Inc.
#
"""log useful diagnostics and file a task to source control oncall"""

from mercurial.i18n import _
from mercurial import cmdutil, util, commands, bookmarks, ui, extensions
from hgext import blackbox
import smartlog
import os, socket, re, time

cmdtable = {}
command = cmdutil.command(cmdtable)

@command('^rage', [], _('hg rage'))
def rage(ui, repo):
    """log useful diagnostics and file a task to source control oncall

    The rage command is for logging useful diagnostics about various
    environment information and filing a task to the source control oncall.

    The following basic information is included in the task description:

    - unixname
    - hostname
    - repo location
    - active bookmark

    Your configured editor will be invoked to let you edit the task title
    and description.

    The following detailed information is uploaded to a Phabricator paste:

    - all the basic information (see above)
    - 'df -h' output
    - 'hg sl' output
    - 'hg config'
    - first 20 lines of 'hg status'
    - last 20 events from 'hg blackbox' ('hg blackbox -l 20')
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

    def shcmd(cmd, input=None, check=True):
            _, _, _, p = util.popen4(cmd)
            out, err = p.communicate(input)
            if check and p.returncode:
                raise util.Abort(cmd + ' error: ' + err)
            return out

    basic = [
        ('date', time.ctime()),
        ('unixname', os.getlogin()),
        ('hostname', socket.gethostname()),
        ('repo location', repo.root),
        ('active bookmark', bookmarks.readactive(repo)),
    ]

    ui._colormode = None

    detailed = [
        ('df -h', shcmd('df -h', check=False)),
        ('hg sl', hgcmd(smartlog.smartlog)),
        ('hg config', hgcmd(commands.config)),
        ('first 20 lines of "hg status"',
            '\n'.join(hgcmd(commands.status).splitlines()[:20])),
        ('hg blackbox -l20', hgcmd(blackbox.blackbox, limit=20)),
    ]

    basic_msg = '\n'.join(map(format, basic))
    prompt = '''Title: [hg rage] %s on %s by %s

Description:

%s
HG: Edit task title and description. Lines beginning with 'HG:' are removed."
HG: First line is the title followed by the description.
HG: Feel free to add relevant information.
''' % (repo.root, socket.gethostname(), os.getlogin(), basic_msg)

    text = re.sub("(?m)^HG:.*(\n|$)", "", ui.edit(prompt, ui.username()))
    lines = text.splitlines()
    title = re.sub("(?m)^Title:\s+", "", lines[0])
    desc = re.sub("(?m)^Description:\s+", "", '\n'.join(lines[1:]))
    detailed_msg = desc + '\n'.join(map(lambda x: format(x, False), detailed))

    print 'pasting the rage info:'
    paste_url = shcmd('arc paste', detailed_msg).split()[1]
    print paste_url

    desc += '\ndetailed output @ ' + paste_url
    tag = 'hg rage'
    oncall = shcmd('oncalls --output unixname source_control').strip()

    print 'filing a task for the oncall %s:' % oncall
    task_id = shcmd(' '.join([
        'tasks',
        'create',
        '--title=' + util.shellquote(title),
        '--pri=low',
        '--assign=' + oncall,
        '--sub=' + oncall,
        '--tag=' + util.shellquote(tag),
        '--desc=' + util.shellquote(desc),
        ])
    )

    task_num = shcmd('tasks view ' + task_id).splitlines()[0].split()[0]
    print 'https://our.intern.facebook.com/intern/tasks/?t=' + task_num
