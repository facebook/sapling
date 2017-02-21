# color.py color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''colorize output from some commands

The color extension colorizes output from several Mercurial commands.
For example, the diff command shows additions in green and deletions
in red, while the status command shows modified files in magenta. Many
other commands have analogous colors. It is possible to customize
these colors.

Effects
-------

Other effects in addition to color, like bold and underlined text, are
also available. By default, the terminfo database is used to find the
terminal codes used to change color and effect.  If terminfo is not
available, then effects are rendered with the ECMA-48 SGR control
function (aka ANSI escape codes).

The available effects in terminfo mode are 'blink', 'bold', 'dim',
'inverse', 'invisible', 'italic', 'standout', and 'underline'; in
ECMA-48 mode, the options are 'bold', 'inverse', 'italic', and
'underline'.  How each is rendered depends on the terminal emulator.
Some may not be available for a given terminal type, and will be
silently ignored.

If the terminfo entry for your terminal is missing codes for an effect
or has the wrong codes, you can add or override those codes in your
configuration::

  [color]
  terminfo.dim = \E[2m

where '\E' is substituted with an escape character.

Labels
------

Text receives color effects depending on the labels that it has. Many
default Mercurial commands emit labelled text. You can also define
your own labels in templates using the label function, see :hg:`help
templates`. A single portion of text may have more than one label. In
that case, effects given to the last label will override any other
effects. This includes the special "none" effect, which nullifies
other effects.

Labels are normally invisible. In order to see these labels and their
position in the text, use the global --color=debug option. The same
anchor text may be associated to multiple labels, e.g.

  [log.changeset changeset.secret|changeset:   22611:6f0a53c8f587]

The following are the default effects for some default labels. Default
effects may be overridden from your configuration file::

  [color]
  status.modified = blue bold underline red_background
  status.added = green bold
  status.removed = red bold blue_background
  status.deleted = cyan bold underline
  status.unknown = magenta bold underline
  status.ignored = black bold

  # 'none' turns off all effects
  status.clean = none
  status.copied = none

  qseries.applied = blue bold underline
  qseries.unapplied = black bold
  qseries.missing = red bold

  diff.diffline = bold
  diff.extended = cyan bold
  diff.file_a = red bold
  diff.file_b = green bold
  diff.hunk = magenta
  diff.deleted = red
  diff.inserted = green
  diff.changed = white
  diff.tab =
  diff.trailingwhitespace = bold red_background

  # Blank so it inherits the style of the surrounding label
  changeset.public =
  changeset.draft =
  changeset.secret =

  resolve.unresolved = red bold
  resolve.resolved = green bold

  bookmarks.active = green

  branches.active = none
  branches.closed = black bold
  branches.current = green
  branches.inactive = none

  tags.normal = green
  tags.local = black bold

  rebase.rebased = blue
  rebase.remaining = red bold

  shelve.age = cyan
  shelve.newest = green bold
  shelve.name = blue bold

  histedit.remaining = red bold

Custom colors
-------------

Because there are only eight standard colors, this module allows you
to define color names for other color slots which might be available
for your terminal type, assuming terminfo mode.  For instance::

  color.brightblue = 12
  color.pink = 207
  color.orange = 202

to set 'brightblue' to color slot 12 (useful for 16 color terminals
that have brighter colors defined in the upper eight) and, 'pink' and
'orange' to colors in 256-color xterm's default color cube.  These
defined colors may then be used as any of the pre-defined eight,
including appending '_background' to set the background to that color.

Modes
-----

By default, the color extension will use ANSI mode (or win32 mode on
Windows) if it detects a terminal. To override auto mode (to enable
terminfo mode, for example), set the following configuration option::

  [color]
  mode = terminfo

Any value other than 'ansi', 'win32', 'terminfo', or 'auto' will
disable color.

Note that on some systems, terminfo mode may cause problems when using
color with the pager extension and less -R. less with the -R option
will only display ECMA-48 color codes, and terminfo mode may sometimes
emit codes that less doesn't understand. You can work around this by
either using ansi mode (or auto mode), or by using less -r (which will
pass through all terminal control codes, not just color control
codes).

On some systems (such as MSYS in Windows), the terminal may support
a different color mode than the pager (activated via the "pager"
extension). It is possible to define separate modes depending on whether
the pager is active::

  [color]
  mode = auto
  pagermode = ansi

If ``pagermode`` is not defined, the ``mode`` will be used.
'''

from __future__ import absolute_import

try:
    import curses
    curses.COLOR_BLACK # force import
except ImportError:
    curses = None

from mercurial.i18n import _
from mercurial import (
    cmdutil,
    color,
    commands,
    dispatch,
    extensions,
    ui as uimod,
)

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def uisetup(ui):
    def colorcmd(orig, ui_, opts, cmd, cmdfunc):
        mode = color._modesetup(ui_, opts['color'])
        uimod.ui._colormode = mode
        if mode and mode != 'debug':
            color.configstyles(ui_)
        return orig(ui_, opts, cmd, cmdfunc)
    extensions.wrapfunction(dispatch, '_runcommand', colorcmd)

def extsetup(ui):
    commands.globalopts.append(
        ('', 'color', 'auto',
         # i18n: 'always', 'auto', 'never', and 'debug' are keywords
         # and should not be translated
         _("when to colorize (boolean, always, auto, never, or debug)"),
         _('TYPE')))

@command('debugcolor',
        [('', 'style', None, _('show all configured styles'))],
        'hg debugcolor')
def debugcolor(ui, repo, **opts):
    """show available color, effects or style"""
    ui.write(('color mode: %s\n') % ui._colormode)
    if opts.get('style'):
        return _debugdisplaystyle(ui)
    else:
        return _debugdisplaycolor(ui)

def _debugdisplaycolor(ui):
    oldstyle = color._styles.copy()
    try:
        color._styles.clear()
        for effect in color._effects.keys():
            color._styles[effect] = effect
        if color._terminfo_params:
            for k, v in ui.configitems('color'):
                if k.startswith('color.'):
                    color._styles[k] = k[6:]
                elif k.startswith('terminfo.'):
                    color._styles[k] = k[9:]
        ui.write(_('available colors:\n'))
        # sort label with a '_' after the other to group '_background' entry.
        items = sorted(color._styles.items(),
                       key=lambda i: ('_' in i[0], i[0], i[1]))
        for colorname, label in items:
            ui.write(('%s\n') % colorname, label=label)
    finally:
        color._styles.clear()
        color._styles.update(oldstyle)

def _debugdisplaystyle(ui):
    ui.write(_('available style:\n'))
    width = max(len(s) for s in color._styles)
    for label, effects in sorted(color._styles.items()):
        ui.write('%s' % label, label=label)
        if effects:
            # 50
            ui.write(': ')
            ui.write(' ' * (max(0, width - len(label))))
            ui.write(', '.join(ui.label(e, e) for e in effects.split()))
        ui.write('\n')
