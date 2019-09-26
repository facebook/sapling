# chistedit.py
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
an interactive ncurses interface to histedit

This extensions allows you to interactively move around changesets
or change the action to perform while keeping track of possible
conflicts.

Use up/down or j/k to move up and down. Select a changeset via space and move
it around. You can use d/e/f/m/r to change the action of a changeset. You
can cycle through available commands with left/h or right/l.

The diff for the current changeset can be viewed by pressing v. To apply
the commands press C, which will call histedit.

The current conflict detection mechanism is based on a per-file
comparison. Reordered changesets that touch the sames files are
considered a "potential conflict".  Please note that Mercurial's merge
algorithm might still be able to merge these files without conflict.
"""

from __future__ import print_function

import functools
import os

from edenscm.mercurial import cmdutil, destutil, error, node, registrar, scmutil, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import range

from . import histedit


try:
    import curses
except ImportError:
    curses = None

KEY_LIST = ["pick", "edit", "fold", "drop", "mess", "roll"]
ACTION_LABELS = {"fold": "^fold", "roll": "^roll"}

COLOR_HELP, COLOR_SELECTED, COLOR_OK, COLOR_WARN = 1, 2, 3, 4

E_QUIT, E_HISTEDIT = 1, 2
E_PAGEDOWN, E_PAGEUP, E_LINEUP, E_LINEDOWN, E_RESIZE = 3, 4, 5, 6, 7
MODE_INIT, MODE_PATCH, MODE_RULES, MODE_HELP = 0, 1, 2, 3

KEYTABLE = {
    "global": {
        "h": "next-action",
        "KEY_RIGHT": "next-action",
        "l": "prev-action",
        "KEY_LEFT": "prev-action",
        "q": "quit",
        "c": "histedit",
        "C": "histedit",
        "v": "showpatch",
        "?": "help",
    },
    MODE_RULES: {
        "d": "action-drop",
        "e": "action-edit",
        "f": "action-fold",
        "m": "action-mess",
        "p": "action-pick",
        "r": "action-roll",
        " ": "select",
        "j": "down",
        "k": "up",
        "KEY_DOWN": "down",
        "KEY_UP": "up",
        "J": "move-down",
        "K": "move-up",
        "KEY_NPAGE": "move-down",
        "KEY_PPAGE": "move-up",
        "0": "goto",  # Used for 0..9
    },
    MODE_PATCH: {
        " ": "page-down",
        "KEY_NPAGE": "page-down",
        "KEY_PPAGE": "page-up",
        "j": "line-down",
        "k": "line-up",
        "KEY_DOWN": "line-down",
        "KEY_UP": "line-up",
        "J": "down",
        "K": "up",
    },
    MODE_HELP: {},
}


def screen_size():
    import termios
    from fcntl import ioctl
    from struct import unpack

    return unpack("hh", ioctl(1, termios.TIOCGWINSZ, "    "))


class histeditrule(object):
    def __init__(self, ctx, pos, action="pick"):
        self.ctx = ctx
        self.action = action
        self.origpos = pos
        self.pos = pos
        self.conflicts = []

    def __str__(self):
        # Some actions ('fold' and 'roll') combine a patch with a previous one.
        # Add a marker showing which patch they apply to, and also omit the
        # description for 'roll' (since it will get discarded). Example display:
        #
        #  #10 pick   316392:06a16c25c053   add option to skip tests
        #  #11 ^roll  316393:71313c964cc5
        #  #12 pick   316394:ab31f3973b0d   include mfbt for mozilla-config.h
        #  #13 ^fold  316395:14ce5803f4c3   fix warnings
        #
        # The carets point to the changeset being folded into ("roll this
        # changeset into the changeset above").
        action = ACTION_LABELS.get(self.action, self.action)
        h = self.ctx.hex()[0:12]
        r = self.ctx.rev()
        desc = self.ctx.description().splitlines()[0].strip()
        if self.action == "roll":
            desc = ""
        return "#{0:<2} {1:<6} {2}:{3}   {4}".format(self.origpos, action, r, h, desc)

    def checkconflicts(self, other):
        if other.pos > self.pos and other.origpos <= self.origpos:
            if set(other.ctx.files()) & set(self.ctx.files()) != set():
                self.conflicts.append(other)
                return self.conflicts

        if other in self.conflicts:
            self.conflicts.remove(other)
        return self.conflicts


# ============ EVENTS ===============
def movecursor(state, oldpos, newpos):
    """Change the rule/changeset that the cursor is pointing to, regardless of
    current mode (you can switch between patches from the view patch window)."""
    state["pos"] = newpos

    mode, _ = state["mode"]
    if mode == MODE_RULES:
        # Scroll through the list by updating the view for MODE_RULES, so that
        # even if we are not currently viewing the rules, switching back will
        # result in the cursor's rule being visible.
        modestate = state["modes"][MODE_RULES]
        if newpos < modestate["line_offset"]:
            modestate["line_offset"] = newpos
        elif newpos > modestate["line_offset"] + state["page_height"] - 1:
            modestate["line_offset"] = newpos - state["page_height"] + 1

    # Reset the patch view region to the top of the new patch.
    state["modes"][MODE_PATCH]["line_offset"] = 0


def changemode(state, mode):
    curmode, _ = state["mode"]
    state["mode"] = (mode, curmode)


def makeselection(state, pos):
    state["selected"] = pos


def swap(state, oldpos, newpos):
    """Swap two positions and calculate necessary conflicts in
    O(|newpos-oldpos|) time"""

    rules = state["rules"]
    assert 0 <= oldpos < len(rules) and 0 <= newpos < len(rules)

    rules[oldpos], rules[newpos] = rules[newpos], rules[oldpos]

    # TODO: swap should not know about histeditrule's internals
    rules[newpos].pos = newpos
    rules[oldpos].pos = oldpos

    start = min(oldpos, newpos)
    end = max(oldpos, newpos)
    for r in range(start, end + 1):
        rules[newpos].checkconflicts(rules[r])
        rules[oldpos].checkconflicts(rules[r])

    if state["selected"]:
        makeselection(state, newpos)


def changeaction(state, pos, action):
    """Change the action state on the given position to the new action"""
    rules = state["rules"]
    assert 0 <= pos < len(rules)
    rules[pos].action = action


def cycleaction(state, pos, next=False):
    """Changes the action state the next or the previous action from
    the action list"""
    rules = state["rules"]
    assert 0 <= pos < len(rules)
    current = rules[pos].action

    assert current in KEY_LIST

    index = KEY_LIST.index(current)
    if next:
        index += 1
    else:
        index -= 1
    changeaction(state, pos, KEY_LIST[index % len(KEY_LIST)])


def changeview(state, delta, unit):
    """Change the region of whatever is being viewed (a patch or the list of
    changesets). 'delta' is an amount (+/- 1) and 'unit' is 'page' or 'line'."""
    mode, _ = state["mode"]
    if mode != MODE_PATCH:
        return
    mode_state = state["modes"][mode]
    num_lines = len(patchcontents(state))
    page_height = state["page_height"]
    unit = page_height if unit == "page" else 1
    num_pages = 1 + (num_lines - 1) / page_height
    max_offset = (num_pages - 1) * page_height
    newline = mode_state["line_offset"] + delta * unit
    mode_state["line_offset"] = max(0, min(max_offset, newline))


def event(state, ch):
    """Change state based on the current character input

    This takes the current state and based on the current character input from
    the user we change the state.
    """
    selected = state["selected"]
    oldpos = state["pos"]
    rules = state["rules"]

    if ch in (curses.KEY_RESIZE, "KEY_RESIZE"):
        return E_RESIZE

    lookup_ch = ch
    if "0" <= ch <= "9":
        lookup_ch = "0"

    curmode, prevmode = state["mode"]
    action = KEYTABLE[curmode].get(lookup_ch, KEYTABLE["global"].get(lookup_ch))
    if action is None:
        return
    if action in ("down", "move-down"):
        newpos = min(oldpos + 1, len(rules) - 1)
        movecursor(state, oldpos, newpos)
        if selected is not None or action == "move-down":
            swap(state, oldpos, newpos)
    elif action in ("up", "move-up"):
        newpos = max(0, oldpos - 1)
        movecursor(state, oldpos, newpos)
        if selected is not None or action == "move-up":
            swap(state, oldpos, newpos)
    elif action == "next-action":
        cycleaction(state, oldpos, next=True)
    elif action == "prev-action":
        cycleaction(state, oldpos, next=False)
    elif action == "select":
        selected = oldpos if selected is None else None
        makeselection(state, selected)
    elif action == "goto" and int(ch) < len(rules) and len(rules) <= 10:
        newrule = next((r for r in rules if r.origpos == int(ch)))
        movecursor(state, oldpos, newrule.pos)
        if selected is not None:
            swap(state, oldpos, newrule.pos)
    elif action.startswith("action-"):
        changeaction(state, oldpos, action[7:])
    elif action == "showpatch":
        changemode(state, MODE_PATCH if curmode != MODE_PATCH else prevmode)
    elif action == "help":
        changemode(state, MODE_HELP if curmode != MODE_HELP else prevmode)
    elif action == "quit":
        return E_QUIT
    elif action == "histedit":
        return E_HISTEDIT
    elif action == "page-down":
        return E_PAGEDOWN
    elif action == "page-up":
        return E_PAGEUP
    elif action == "line-down":
        return E_LINEDOWN
    elif action == "line-up":
        return E_LINEUP


def makecommands(rules):
    """Returns a list of commands consumable by histedit --commands based on
    our list of rules"""
    commands = []
    for rules in rules:
        commands.append("{0} {1}\n".format(rules.action, rules.ctx))
    return commands


def addln(win, y, x, line, color=None):
    """Add a line to the given window left padding but 100% filled with
    whitespace characters, so that the color appears on the whole line"""
    maxy, maxx = win.getmaxyx()
    length = maxx - 1 - x
    line = ("{0:<%d}" % length).format(str(line).strip())[:length]
    if y < 0:
        y = maxy + y
    if x < 0:
        x = maxx + x
    if color:
        win.addstr(y, x, line, color)
    else:
        win.addstr(y, x, line)


def patchcontents(state):
    repo = state["repo"]
    rule = state["rules"][state["pos"]]
    displayer = cmdutil.show_changeset(
        repo.ui, repo, {"patch": True, "verbose": True}, buffered=True
    )
    displayer.show(rule.ctx)
    displayer.close()
    return displayer.hunk[rule.ctx.rev()].splitlines()


def main(repo, rules, stdscr):
    # initialize color pattern
    curses.init_pair(COLOR_HELP, curses.COLOR_WHITE, curses.COLOR_BLUE)
    curses.init_pair(COLOR_SELECTED, curses.COLOR_BLACK, curses.COLOR_WHITE)
    curses.init_pair(COLOR_WARN, curses.COLOR_BLACK, curses.COLOR_YELLOW)
    curses.init_pair(COLOR_OK, curses.COLOR_BLACK, curses.COLOR_GREEN)

    # don't display the cursor
    try:
        curses.curs_set(0)
    except curses.error:
        pass

    def rendercommit(win, state):
        """Renders the commit window that shows the log of the current selected
        commit"""
        pos = state["pos"]
        rules = state["rules"]
        rule = rules[pos]

        ctx = rule.ctx
        win.box()

        maxy, maxx = win.getmaxyx()
        length = maxx - 3

        line = "changeset: {0}:{1:<12}".format(ctx.rev(), ctx)
        win.addstr(1, 1, line[:length])

        line = "user:      {0}".format(util.shortuser(ctx.user()))
        win.addstr(2, 1, line[:length])

        bms = repo.nodebookmarks(ctx.node())
        line = "bookmark:  {0}".format(" ".join(bms))
        win.addstr(3, 1, line[:length])

        line = "files:     {0}".format(",".join(ctx.files()))
        win.addstr(4, 1, line[:length])

        line = "summary:   {0}".format(ctx.description().splitlines()[0])
        win.addstr(5, 1, line[:length])

        conflicts = rule.conflicts
        if len(conflicts) > 0:
            conflictstr = ",".join(map(lambda r: str(r.ctx), conflicts))
            conflictstr = "changed files overlap with {0}".format(conflictstr)
        else:
            conflictstr = "no overlap"

        win.addstr(6, 1, conflictstr[:length])
        win.noutrefresh()

    def helplines(mode):
        if mode == MODE_PATCH:
            help = """\
?: help, k/up: line up, j/down: line down, v: stop viewing patch
pgup: prev page, space/pgdn: next page, c: commit, q: abort
"""
        else:
            help = """\
?: help, k/up: move up, j/down: move down, space: select, v: view patch
d: drop, e: edit, f: fold, m: mess, p: pick, r: roll
pgup/K: move patch up, pgdn/J: move patch down, c: commit, q: abort
"""
        return help.splitlines()

    def renderhelp(win, state):
        maxy, maxx = win.getmaxyx()
        mode, _ = state["mode"]
        for y, line in enumerate(helplines(mode)):
            if y >= maxy:
                break
            addln(win, y, 0, line, curses.color_pair(COLOR_HELP))
        win.noutrefresh()

    def renderrules(rulesscr, state):
        rules = state["rules"]
        pos = state["pos"]
        selected = state["selected"]
        start = state["modes"][MODE_RULES]["line_offset"]

        conflicts = [r.ctx for r in rules if r.conflicts]
        if len(conflicts) > 0:
            line = "potential conflict in %s" % ",".join(map(str, conflicts))
            addln(rulesscr, -1, 0, line, curses.color_pair(COLOR_WARN))

        for y, rule in enumerate(rules[start:]):
            if y >= state["page_height"]:
                break
            if len(rule.conflicts) > 0:
                rulesscr.addstr(y, 0, " ", curses.color_pair(COLOR_WARN))
            else:
                rulesscr.addstr(y, 0, " ", curses.COLOR_BLACK)
            if y + start == selected:
                addln(rulesscr, y, 2, rule, curses.color_pair(COLOR_SELECTED))
            elif y + start == pos:
                addln(rulesscr, y, 2, rule, curses.A_BOLD)
            else:
                addln(rulesscr, y, 2, rule)
        rulesscr.noutrefresh()

    def renderstring(win, state, output):
        maxy, maxx = win.getmaxyx()
        length = min(maxy - 1, len(output))
        for y in range(0, length):
            win.addstr(y, 0, output[y])
        win.noutrefresh()

    def renderpatch(win, state):
        start = state["modes"][MODE_PATCH]["line_offset"]
        renderstring(win, state, patchcontents(state)[start:])

    def layout(mode):
        maxy, maxx = stdscr.getmaxyx()
        helplen = len(helplines(mode))
        return {
            "commit": (8, maxx),
            "help": (helplen, maxx),
            "main": (maxy - helplen - 8, maxx),
        }

    def drawvertwin(size, y, x):
        win = curses.newwin(size[0], size[1], y, x)
        y += size[0]
        return win, y, x

    state = {
        "pos": 0,
        "rules": rules,
        "selected": None,
        "mode": (MODE_INIT, MODE_INIT),
        "page_height": None,
        "modes": {MODE_RULES: {"line_offset": 0}, MODE_PATCH: {"line_offset": 0}},
        "repo": repo,
    }

    # eventloop
    ch = None
    stdscr.clear()
    stdscr.refresh()
    while True:
        try:
            oldmode, _ = state["mode"]
            if oldmode == MODE_INIT:
                changemode(state, MODE_RULES)
            e = event(state, ch)

            if e == E_QUIT:
                return False
            if e == E_HISTEDIT:
                return state["rules"]
            else:
                if e == E_RESIZE:
                    size = screen_size()
                    if size != stdscr.getmaxyx():
                        curses.resizeterm(*size)

                curmode, _ = state["mode"]
                sizes = layout(curmode)
                if curmode != oldmode:
                    state["page_height"] = sizes["main"][0]
                    # Adjust the view to fit the current screen size.
                    movecursor(state, state["pos"], state["pos"])

                # Pack the windows against the top, each pane spread across the
                # full width of the screen.
                y, x = (0, 0)
                helpwin, y, x = drawvertwin(sizes["help"], y, x)
                mainwin, y, x = drawvertwin(sizes["main"], y, x)
                commitwin, y, x = drawvertwin(sizes["commit"], y, x)

                if e in (E_PAGEDOWN, E_PAGEUP, E_LINEDOWN, E_LINEUP):
                    if e == E_PAGEDOWN:
                        changeview(state, +1, "page")
                    elif e == E_PAGEUP:
                        changeview(state, -1, "page")
                    elif e == E_LINEDOWN:
                        changeview(state, +1, "line")
                    elif e == E_LINEUP:
                        changeview(state, -1, "line")

                # start rendering
                commitwin.erase()
                helpwin.erase()
                mainwin.erase()
                if curmode == MODE_PATCH:
                    renderpatch(mainwin, state)
                elif curmode == MODE_HELP:
                    renderstring(mainwin, state, __doc__.strip().splitlines())
                else:
                    renderrules(mainwin, state)
                    rendercommit(commitwin, state)
                renderhelp(helpwin, state)
                curses.doupdate()
                # done rendering
                ch = stdscr.getkey()
        except curses.error:
            pass


cmdtable = {}
command = registrar.command(cmdtable)

testedwith = "ships-with-fb-hgext"


@command(
    "chistedit",
    [
        ("k", "keep", False, _("don't strip old nodes after edit is complete")),
        ("r", "rev", [], _("first revision to be edited")),
    ],
    _("[OPTION]... [ANCESTOR]"),
)
def chistedit(ui, repo, *freeargs, **opts):
    """Provides a ncurses interface to histedit. Press ? in chistedit mode
    to see an extensive help. Requires python-curses to be installed."""

    if curses is None:
        raise error.Abort(_("Python curses library required"))

    # disable color
    ui._colormode = None

    try:
        keep = opts.get("keep")
        revs = opts.get("rev", [])[:]
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)

        if os.path.exists(os.path.join(repo.path, "histedit-state")):
            raise error.Abort(
                _("history edit already in progress, try " "--continue or --abort")
            )
        revs.extend(freeargs)
        if not revs:
            defaultrev = destutil.desthistedit(ui, repo)
            if defaultrev is not None:
                revs.append(defaultrev)
        if len(revs) != 1:
            raise error.Abort(_("histedit requires exactly one ancestor revision"))

        rr = list(repo.set("roots(%ld)", scmutil.revrange(repo, revs)))
        if len(rr) != 1:
            raise error.Abort(
                _("The specified revisions must have " "exactly one common root")
            )
        root = rr[0].node()

        topmost, empty = repo.dirstate.parents()
        revs = histedit.between(repo, root, topmost, keep)
        if not revs:
            raise error.Abort(
                _("%s is not an ancestor of working directory") % node.short(root)
            )

        ctxs = []
        for i, r in enumerate(revs):
            ctxs.append(histeditrule(repo[r], i))
        rc = curses.wrapper(functools.partial(main, repo, ctxs))
        curses.echo()
        curses.endwin()
        if rc is False:
            ui.write(_("chistedit aborted\n"))
            return 0
        if type(rc) is list:
            ui.status(_("running histedit\n"))
            rules = makecommands(rc)
            filename = repo.localvfs.join("chistedit")
            with open(filename, "w+") as fp:
                for r in rules:
                    fp.write(r)
            opts["commands"] = filename
            return histedit.histedit(ui, repo, *freeargs, **opts)
    except KeyboardInterrupt:
        pass
    return -1
