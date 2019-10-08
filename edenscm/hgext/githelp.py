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

If an unknown command or parameter combination is detected, an error is
produced, followed by a footer with instructions on how to contact the
maintainers if the command is legitimate. To customize this footer, set:

  [githelp]
  unknown.footer = My footer message

(newlines are possible in hgrc by indenting the lines after the first)

"""
import getopt
import re

from edenscm.mercurial import error, extensions, fancyopts, registrar, util
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"


class GitUnknownError(error.Abort):
    defaultfooter = (
        "If this is a valid git command, please search/ask in the Source "
        "Control @ FB group (and don't forget to tell us what the git command "
        "does)."
    )

    def __init__(self, ui, msg):
        footer = ui.config("githelp", "unknown.footer", GitUnknownError.defaultfooter)
        if footer:
            msg = msg + "\n\n" + footer
        super(GitUnknownError, self).__init__(msg)


def convert(s):
    if s.startswith("origin/"):
        return s[7:]
    if "HEAD" in s:
        s = s.replace("HEAD", ".")
    # HEAD~ in git is .~1 in mercurial
    s = re.sub("~$", "~1", s)
    return s


@command("githelp|git", [], _("-- GIT COMMAND"))
def githelp(ui, repo, *args, **kwargs):
    """suggests the Mercurial equivalent of the given git command

    Usage: hg githelp -- <git command>
    """

    if len(args) == 0 or (len(args) == 1 and args[0] == "git"):
        raise error.Abort(
            _("missing git command - " "usage: hg githelp -- <git command>")
        )

    if args[0] == "git":
        args = args[1:]

    cmd = args[0]
    if not cmd in gitcommands:
        raise GitUnknownError(ui, "error: unknown git command %s" % (cmd))

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
        except getopt.GetoptError as ex:
            flag = None
            if "requires argument" in ex.msg:
                raise
            if ("--" + ex.opt) in ex.msg:
                flag = "--" + ex.opt
            elif ("-" + ex.opt) in ex.msg:
                flag = "-" + ex.opt
            else:
                raise GitUnknownError(ui, "unknown option %s" % ex.opt)
            try:
                args.remove(flag)
            except Exception:
                raise GitUnknownError(
                    ui,
                    "unknown option {0} packed with other options\n"
                    "Please try passing the option as it's own flag: -{0}".format(
                        ex.opt
                    ),
                )

            ui.warn(_("ignoring unknown option %s\n") % flag)

    args = list([convert(x) for x in args])
    opts = dict(
        [(k, convert(v)) if isinstance(v, str) else (k, v) for k, v in opts.iteritems()]
    )

    return args, opts


class Command(object):
    def __init__(self, name):
        self.name = name
        self.args = []
        self.opts = {}

    def __str__(self):
        cmd = "hg " + self.name
        if self.opts:
            for k, values in sorted(self.opts.iteritems()):
                for v in values:
                    if v:
                        cmd += " %s %s" % (k, v)
                    else:
                        cmd += " %s" % (k,)
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
    cmdoptions = [("A", "all", None, ""), ("p", "patch", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get("patch"):
        ui.status(_("note: hg crecord has a better UI to record changes\n"))
        ui.status(
            _(
                "note: record and crecord will commit when complete, "
                "as there is no staging area in mercurial\n\n"
            )
        )
        cmd = Command("record")
    else:
        cmd = Command("add")

        if not opts.get("all"):
            cmd.extend(args)
        else:
            ui.status(
                _(
                    "note: use hg addremove to remove files that have "
                    "been deleted.\n\n"
                )
            )
        if not opts.get("all"):
            cmd.extend(args)
        else:
            ui.status(
                _(
                    "note: use hg addremove to remove files that have "
                    "been deleted.\n\n"
                )
            )

    ui.status((str(cmd)), "\n")


def am(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)
    cmd = Command("mimport -m")
    ui.status(str(cmd), "\n\n")
    ui.status(_("note: requires the MboxExtension and the MqExtension.\n"))


def apply(ui, repo, *args, **kwargs):
    cmdoptions = [("p", "p", int, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("import --no-commit")
    if opts.get("p"):
        cmd["-p"] = opts.get("p")
    cmd.extend(args)

    ui.status((str(cmd)), "\n")


def bisect(ui, repo, *args, **kwargs):
    ui.status(_("See 'hg help bisect' for how to use bisect.\n\n"))


def blame(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)
    try:
        # If tweakdefaults is enabled then we have access to -p, which adds
        # Phabricator diff ID
        extensions.find("tweakdefaults")
        cmd = Command("annotate -pudl")
    except KeyError:
        cmd = Command("annotate -udl")
    cmd.extend([convert(v) for v in args])
    ui.status((str(cmd)), "\n")


def branch(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "set-upstream", None, ""),
        ("", "set-upstream-to", "", ""),
        ("d", "delete", None, ""),
        ("D", "delete", None, ""),
        ("m", "move", None, ""),
        ("M", "move", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("bookmark")

    if opts.get("set_upstream") or opts.get("set_upstream_to"):
        ui.status(_("Mercurial has no concept of upstream branches\n"))
        return
    elif opts.get("delete"):
        cmd = Command("hide")
        if args:
            for branch in args:
                cmd["-B"] = branch
        else:
            cmd["-B"] = None
    elif opts.get("move"):
        if len(args) > 0:
            if len(args) > 1:
                old = args.pop(0)
            else:
                # shell command to output the active bookmark for the active
                # revision
                old = '`hg log -T"{activebookmark}" -r .`'
        new = args[0]
        cmd["-m"] = old
        cmd.append(new)
    else:
        if len(args) > 1:
            cmd["-r"] = args[1]
            cmd.append(args[0])
        elif len(args) == 1:
            cmd.append(args[0])
    ui.status((str(cmd)), "\n")


def ispath(repo, string):
    """
    The first argument to git checkout can either be a revision or a path. Let's
    generally assume it's a revision, unless it's obviously a path. There are
    too many ways to spell revisions in git for us to reasonably catch all of
    them, so let's be conservative.
    """
    if string in repo:
        # if it's definitely a revision let's not even check if a file of the
        # same name exists.
        return False

    cwd = repo.getcwd()
    if cwd == "":
        repopath = string
    else:
        repopath = cwd + "/" + string

    exists = repo.wvfs.exists(repopath)
    if exists:
        return True

    manifest = repo["."].manifest()

    didexist = (repopath in manifest) or manifest.hasdir(repopath)

    return didexist


def checkout(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("b", "branch", "", ""),
        ("B", "branch", "", ""),
        ("f", "force", None, ""),
        ("p", "patch", None, ""),
    ]
    paths = []
    if "--" in args:
        sepindex = args.index("--")
        paths.extend(args[sepindex + 1 :])
        args = args[:sepindex]

    args, opts = parseoptions(ui, cmdoptions, args)

    rev = None
    if args and ispath(repo, args[0]):
        paths = args + paths
    elif args:
        rev = args[0]
        paths = args[1:] + paths

    cmd = Command("update")

    if opts.get("force"):
        if paths or rev:
            cmd["-C"] = None

    if opts.get("patch"):
        cmd = Command("revert")
        cmd["-i"] = None

    if opts.get("branch"):
        if len(args) == 0:
            cmd = Command("bookmark")
            cmd.append(opts.get("branch"))
        else:
            cmd.append(args[0])
            bookcmd = Command("bookmark")
            bookcmd.append(opts.get("branch"))
            cmd = cmd & bookcmd
    # if there is any path argument supplied, use revert instead of update
    elif len(paths) > 0:
        ui.status(_("note: use --no-backup to avoid creating .orig files\n\n"))
        cmd = Command("revert")
        if opts.get("patch"):
            cmd["-i"] = None
        if rev:
            cmd["-r"] = rev
        cmd.extend(paths)
    elif rev:
        if opts.get("patch"):
            cmd["-r"] = rev
        else:
            cmd.append(rev)
    elif opts.get("force"):
        cmd = Command("revert")
        cmd["--all"] = None
    else:
        raise GitUnknownError(ui, "a commit must be specified")

    ui.status((str(cmd)), "\n")


def cherrypick(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "continue", None, ""),
        ("", "abort", None, ""),
        ("e", "edit", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("graft")

    if opts.get("edit"):
        cmd["--edit"] = None
    if opts.get("continue"):
        cmd["--continue"] = None
    elif opts.get("abort"):
        cmd["--abort"] = None
    else:
        cmd.extend(args)

    ui.status((str(cmd)), "\n")


def clean(ui, repo, *args, **kwargs):
    cmdoptions = [("d", "d", None, ""), ("f", "force", None, ""), ("x", "x", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("purge")
    if opts.get("x"):
        cmd["--all"] = None
    cmd.extend(args)

    ui.status((str(cmd)), "\n")


def clone(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "bare", None, ""),
        ("n", "no-checkout", None, ""),
        ("b", "branch", "", ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if len(args) == 0:
        raise GitUnknownError(ui, "a repository to clone must be specified")

    cmd = Command("clone")
    cmd.append(args[0])
    if len(args) > 1:
        cmd.append(args[1])

    if opts.get("bare"):
        cmd["-U"] = None
        ui.status(
            _(
                "note: Mercurial does not have bare clones. "
                + "-U will clone the repo without checking out a commit\n\n"
            )
        )
    elif opts.get("no_checkout"):
        cmd["-U"] = None

    if opts.get("branch"):
        cocmd = Command("update")
        cocmd.append(opts.get("branch"))
        cmd = cmd & cocmd

    ui.status((str(cmd)), "\n")


def commit(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("a", "all", None, ""),
        ("m", "message", "", ""),
        ("p", "patch", None, ""),
        ("C", "reuse-message", "", ""),
        ("F", "file", "", ""),
        ("", "author", "", ""),
        ("", "date", "", ""),
        ("", "amend", None, ""),
        ("", "no-edit", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("commit")
    if opts.get("patch"):
        cmd = Command("record")

    if opts.get("amend"):
        if opts.get("no_edit"):
            cmd = Command("amend")
        else:
            cmd["--amend"] = None

    if opts.get("reuse_message"):
        cmd["-M"] = opts.get("reuse_message")

    if opts.get("message"):
        cmd["-m"] = "'%s'" % (opts.get("message"),)

    if opts.get("all"):
        ui.status(
            _(
                "note: Mercurial doesn't have a staging area, "
                + "so there is no --all. -A will add and remove files "
                + "for you though.\n\n"
            )
        )

    if opts.get("file"):
        cmd["-l"] = opts.get("file")

    if opts.get("author"):
        cmd["-u"] = opts.get("author")

    if opts.get("date"):
        cmd["-d"] = opts.get("date")

    cmd.extend(args)

    ui.status((str(cmd)), "\n")


def deprecated(ui, repo, *args, **kwargs):
    ui.warn(
        _(
            "This command has been deprecated in the git project, "
            + "thus isn't supported by this tool.\n\n"
        )
    )


def diff(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("a", "all", None, ""),
        ("", "cached", None, ""),
        ("R", "reverse", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("diff")

    if opts.get("cached"):
        ui.status(
            _(
                "note: Mercurial has no concept of a staging area, "
                + "so --cached does nothing.\n\n"
            )
        )

    if opts.get("reverse"):
        cmd["--reverse"] = None

    for a in list(args):
        args.remove(a)
        try:
            repo.revs(a)
            cmd["-r"] = a
        except Exception:
            cmd.append(a)

    ui.status((str(cmd)), "\n")


def difftool(ui, repo, *args, **kwargs):
    ui.status(
        _(
            "Mercurial does not enable external difftool by default. You "
            "need to enable the extdiff extension in your .hgrc file by adding\n"
            "extdiff =\n"
            "to the [extensions] section and then running\n\n"
            "hg extdiff -p <program>\n\n"
            "See 'hg help extdiff' and 'hg help -e extdiff' for more "
            "information.\n"
        )
    )


def fetch(ui, repo, *args, **kwargs):
    cmdoptions = [("", "all", None, ""), ("f", "force", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("pull")

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status(
                _(
                    "note: Mercurial doesn't have refspecs. "
                    + "-r can be used to specify which commits you want to pull. "
                    + "-B can be used to specify which bookmark you want to pull."
                    + "\n\n"
                )
            )
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd["-B"] = v
                else:
                    cmd["-r"] = v

    ui.status((str(cmd)), "\n")


def grep(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("grep")

    # For basic usage, git grep and hg grep are the same. They both have the
    # pattern first, followed by paths.
    cmd.extend(args)

    ui.status((str(cmd)), "\n")


def init(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("init")

    if len(args) > 0:
        cmd.append(args[0])

    ui.status((str(cmd)), "\n")


def log(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "follow", None, ""),
        ("", "decorate", None, ""),
        ("n", "number", "", ""),
        ("1", "1", None, ""),
        ("", "pretty", "", ""),
        ("", "format", "", ""),
        ("", "oneline", None, ""),
        ("", "stat", None, ""),
        ("", "graph", None, ""),
        ("p", "patch", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)
    ui.status(
        _(
            "note: -v prints the entire commit message like Git does. To "
            + "print just the first line, drop the -v.\n\n"
        )
    )
    ui.status(
        _(
            "note: see hg help revset for information on how to filter "
            + "log output.\n\n"
        )
    )

    cmd = Command("log")
    cmd["-v"] = None

    if opts.get("number"):
        cmd["-l"] = opts.get("number")
    if opts.get("1"):
        cmd["-l"] = "1"
    if opts.get("stat"):
        cmd["--stat"] = None
    if opts.get("graph"):
        cmd["-G"] = None
    if opts.get("patch"):
        cmd["-p"] = None

    if opts.get("pretty") or opts.get("format") or opts.get("oneline"):
        format = opts.get("format", "")
        if "format:" in format:
            ui.status(
                _(
                    "note: --format format:??? equates to Mercurial's "
                    + "--template. See hg help templates for more info.\n\n"
                )
            )
            cmd["--template"] = "???"
        else:
            ui.status(
                _(
                    "note: --pretty/format/oneline equate to Mercurial's "
                    + "--style or --template. See hg help templates for more info."
                    + "\n\n"
                )
            )
            cmd["--style"] = "???"

    if len(args) > 0:
        if ".." in args[0]:
            since, until = args[0].split("..")
            cmd["-r"] = "'%s::%s'" % (since, until)
            del args[0]
        cmd.extend(args)

    ui.status((str(cmd)), "\n")


def lsfiles(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("c", "cached", None, ""),
        ("d", "deleted", None, ""),
        ("m", "modified", None, ""),
        ("o", "others", None, ""),
        ("i", "ignored", None, ""),
        ("s", "stage", None, ""),
        ("z", "_zero", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if (
        opts.get("modified")
        or opts.get("deleted")
        or opts.get("others")
        or opts.get("ignored")
    ):
        cmd = Command("status")
        if opts.get("deleted"):
            cmd["-d"] = None
        if opts.get("modified"):
            cmd["-m"] = None
        if opts.get("others"):
            cmd["-o"] = None
        if opts.get("ignored"):
            cmd["-i"] = None
    else:
        cmd = Command("files")
    if opts.get("stage"):
        ui.status(
            _("note: Mercurial doesn't have a staging area, ignoring " "--stage\n")
        )
    if opts.get("_zero"):
        cmd["-0"] = None
    cmd.append(".")
    for include in args:
        cmd["-I"] = util.shellquote(include)

    ui.status((str(cmd)), "\n")


def merge(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("merge")

    if len(args) > 0:
        cmd.append(args[len(args) - 1])

    ui.status((str(cmd)), "\n")


def mergebase(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    if len(args) != 2:
        args = ["A", "B"]

    cmd = Command("log -T '{node}\\n' -r 'ancestor(%s,%s)'" % (args[0], args[1]))

    ui.status(
        _("NOTE: ancestors() is part of the revset language.\n"),
        _("Learn more about revsets with 'hg help revsets'\n\n"),
    )
    ui.status((str(cmd)), "\n")


def mergetool(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("resolve")

    if len(args) == 0:
        cmd["--all"] = None
    cmd.extend(args)
    ui.status((str(cmd)), "\n")


def mv(ui, repo, *args, **kwargs):
    cmdoptions = [("f", "force", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("mv")
    cmd.extend(args)

    if opts.get("force"):
        cmd["-f"] = None

    ui.status((str(cmd)), "\n")


def pull(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "all", None, ""),
        ("f", "force", None, ""),
        ("r", "rebase", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("pull")
    cmd["--rebase"] = None

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status(
                _(
                    "note: Mercurial doesn't have refspecs. "
                    + "-r can be used to specify which commits you want to pull. "
                    + "-B can be used to specify which bookmark you want to pull."
                    + "\n\n"
                )
            )
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd["-B"] = v
                else:
                    cmd["-r"] = v

    ui.status((str(cmd)), "\n")


def push(ui, repo, *args, **kwargs):
    cmdoptions = [("", "all", None, ""), ("f", "force", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("push")

    if len(args) > 0:
        cmd.append(args[0])
        if len(args) > 1:
            ui.status(
                _(
                    "note: Mercurial doesn't have refspecs. "
                    + "-r can be used to specify which commits you want to push. "
                    + "-B can be used to specify which bookmark you want to push."
                    + "\n\n"
                )
            )
            for v in args[1:]:
                if v in repo._bookmarks:
                    cmd["-B"] = v
                else:
                    cmd["-r"] = v

    if opts.get("force"):
        cmd["-f"] = None

    ui.status((str(cmd)), "\n")


def rebase(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "all", None, ""),
        ("i", "interactive", None, ""),
        ("", "onto", "", ""),
        ("", "abort", None, ""),
        ("", "continue", None, ""),
        ("", "skip", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get("skip"):
        cmd = Command("revert --all -r .")
        ui.status((str(cmd)), "\n")

    cmd = Command("rebase")

    if opts.get("interactive"):
        cmd["--interactive"] = None
        ui.status(
            _(
                "note: if you don't need to rebase use 'hg histedit'. "
                + "It just edits history.\n\n"
            )
        )
        if len(args) > 0:
            ui.status(
                _(
                    "also note: 'hg histedit' will automatically detect"
                    " your stack, so no second argument is necessary.\n\n"
                )
            )

    if opts.get("continue") or opts.get("skip"):
        cmd["--continue"] = None
    if opts.get("abort"):
        cmd["--abort"] = None

    if opts.get("onto"):
        ui.status(
            _(
                "note: if you're trying to lift a commit off one branch, "
                + "try hg rebase -d <destination commit> -s <commit to be lifted>"
                + "\n\n"
            )
        )
        cmd["-d"] = convert(opts.get("onto"))
        if len(args) < 2:
            raise GitUnknownError(ui, "Expected format: git rebase --onto X Y Z")
        cmd["-s"] = "'::%s - ::%s'" % (convert(args[1]), convert(args[0]))
    else:
        if len(args) == 1:
            cmd["-d"] = convert(args[0])
        elif len(args) == 2:
            cmd["-d"] = convert(args[0])
            cmd["-b"] = convert(args[1])

    ui.status((str(cmd)), "\n")


def reflog(ui, repo, *args, **kwargs):
    cmdoptions = [("", "all", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("journal")
    if opts.get("all"):
        cmd["--all"] = None
    if len(args) > 0:
        cmd.append(args[0])

    ui.status(str(cmd), "\n\n")
    ui.status(
        _(
            "note: in hg commits can be deleted from repo but we always"
            " have backups.\n"
            "Please use 'hg backups --restore' or 'hg reset'"
            + " to restore from backups.\n"
        )
    )


def reset(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "soft", None, ""),
        ("", "hard", None, ""),
        ("", "mixed", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    commit = convert(args[0] if len(args) > 0 else ".")
    hard = opts.get("hard")

    if opts.get("mixed"):
        ui.status(
            _(
                "NOTE: --mixed has no meaning since mercurial has no "
                + "staging area\n\n"
            )
        )

    cmd = Command("reset")
    if hard:
        cmd.append("--clean")
    cmd.append(commit)

    ui.status((str(cmd)), "\n")


def revert(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    if len(args) > 1:
        ui.status(_("note: hg backout doesn't support multiple commits at once\n\n"))

    cmd = Command("backout")
    if args:
        cmd.append(args[0])

    ui.status((str(cmd)), "\n")


def revparse(ui, repo, *args, **kwargs):
    cmdoptions = [("", "show-cdup", None, ""), ("", "show-toplevel", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get("show_cdup") or opts.get("show_toplevel"):
        cmd = Command("root")
        if opts.get("show_cdup"):
            ui.status(_("note: hg root prints the root of the repository\n\n"))
        ui.status((str(cmd)), "\n")
    else:
        ui.status(_("note: see hg help revset for how to refer to commits\n"))


def rm(ui, repo, *args, **kwargs):
    cmdoptions = [("f", "force", None, ""), ("n", "dry-run", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("rm")
    cmd.extend(args)

    if opts.get("force"):
        cmd["-f"] = None
    if opts.get("dry_run"):
        cmd["-n"] = None

    ui.status((str(cmd)), "\n")


def show(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("", "name-status", None, ""),
        ("", "pretty", "", ""),
        ("U", "unified", int, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("show")
    if opts.get("name_status"):
        if opts.get("pretty") == "format:":
            cmd = Command("stat")
            cmd["--change"] = "tip"
        else:
            cmd = Command("log")
            cmd.append("--style status")
            cmd.append("-r tip")
    elif len(args) > 0:
        if ispath(repo, args[0]):
            cmd.append(".")
        cmd.extend(args)
        if opts.get("unified"):
            cmd.append("--config diff.unified=%d" % (opts["unified"],))
    elif opts.get("unified"):
        cmd.append("--config diff.unified=%d" % (opts["unified"],))

    ui.status((str(cmd)), "\n")


def stash(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("shelve")
    action = args[0] if len(args) > 0 else None

    if action == "list":
        cmd["-l"] = None
    elif action == "drop":
        cmd["-d"] = None
        if len(args) > 1:
            cmd.append(args[1])
        else:
            cmd.append("<shelve name>")
    elif action == "pop" or action == "apply":
        cmd = Command("unshelve")
        if len(args) > 1:
            cmd.append(args[1])
        if action == "apply":
            cmd["--keep"] = None
    elif (
        action == "branch"
        or action == "show"
        or action == "clear"
        or action == "create"
    ):
        ui.status(
            _(
                "note: Mercurial doesn't have equivalents to the "
                + "git stash branch, show, clear, or create actions.\n\n"
            )
        )
        return
    else:
        if len(args) > 0:
            if args[0] != "save":
                cmd["--name"] = args[0]
            elif len(args) > 1:
                cmd["--name"] = args[1]

    ui.status((str(cmd)), "\n")


def status(ui, repo, *args, **kwargs):
    cmdoptions = [("", "ignored", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("status")
    cmd.extend(args)

    if opts.get("ignored"):
        cmd["-i"] = None

    ui.status((str(cmd)), "\n")


def svn(ui, repo, *args, **kwargs):
    svncmd = args[0]
    if not svncmd in gitsvncommands:
        ui.warn(_("error: unknown git svn command %s\n") % (svncmd))

    args = args[1:]
    return gitsvncommands[svncmd](ui, repo, *args, **kwargs)


def svndcommit(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("push")

    ui.status((str(cmd)), "\n")


def svnfetch(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("pull")
    cmd.append("default-push")

    ui.status((str(cmd)), "\n")


def svnfindrev(ui, repo, *args, **kwargs):
    cmdoptions = []
    args, opts = parseoptions(ui, cmdoptions, args)

    cmd = Command("log")
    cmd["-r"] = args[0]

    ui.status((str(cmd)), "\n")


def svnrebase(ui, repo, *args, **kwargs):
    cmdoptions = [("l", "local", None, "")]
    args, opts = parseoptions(ui, cmdoptions, args)

    pullcmd = Command("pull")
    pullcmd.append("default-push")
    rebasecmd = Command("rebase")
    rebasecmd.append("tip")

    cmd = pullcmd & rebasecmd

    ui.status((str(cmd)), "\n")


def tag(ui, repo, *args, **kwargs):
    cmdoptions = [
        ("f", "force", None, ""),
        ("l", "list", None, ""),
        ("d", "delete", None, ""),
    ]
    args, opts = parseoptions(ui, cmdoptions, args)

    if opts.get("list"):
        cmd = Command("tags")
    else:
        cmd = Command("tag")
        cmd.append(args[0])
        if len(args) > 1:
            cmd["-r"] = args[1]

        if opts.get("delete"):
            cmd["--remove"] = None

        if opts.get("force"):
            cmd["-f"] = None

    ui.status((str(cmd)), "\n")


gitcommands = {
    "add": add,
    "am": am,
    "apply": apply,
    "bisect": bisect,
    "blame": blame,
    "branch": branch,
    "checkout": checkout,
    "cherry-pick": cherrypick,
    "clean": clean,
    "clone": clone,
    "commit": commit,
    "diff": diff,
    "difftool": difftool,
    "fetch": fetch,
    "grep": grep,
    "init": init,
    "log": log,
    "ls-files": lsfiles,
    "merge": merge,
    "merge-base": mergebase,
    "mergetool": mergetool,
    "mv": mv,
    "pull": pull,
    "push": push,
    "rebase": rebase,
    "reflog": reflog,
    "reset": reset,
    "revert": revert,
    "rev-parse": revparse,
    "rm": rm,
    "show": show,
    "stash": stash,
    "status": status,
    "svn": svn,
    "tag": tag,
    "whatchanged": deprecated,
}

gitsvncommands = {
    "dcommit": svndcommit,
    "fetch": svnfetch,
    "find-rev": svnfindrev,
    "rebase": svnrebase,
}
