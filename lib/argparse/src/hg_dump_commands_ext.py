# mercurial_dump_commands.py
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""dump all the command definitions from hg so the commandline arg parsing
can be done outside of python code
"""

from mercurial import commands, registrar

cmdtable = {}
command = registrar.command(cmdtable)

def gen_arg(short_name, long_name, default=None, h=None, metavar=None):
    yield 'Arg::with_name("%s")' % long_name
    if len(short_name) > 0:
        yield ".short(b'%s')" % short_name
    # only boolean options don't require values
    if not isinstance(default, bool) and default is not None:
        yield '.requires_value()'

def gen_args(args):
    for arg in args:
        yield '.arg(%s)' % ''.join(gen_arg(*arg))

def gen_subcommand(cmd, impl, args, h=None):
    # aliases are separated with |
    names = cmd.lstrip("^").split("|")
    yield 'Command::with_name("%s")' % names[0]

    for name in names[1:]:
        yield '.alias("%s")'

    # ^ is for hiding in short help
    if not cmd.startswith('^'):
        yield '.help_visibility(HelpVisibility::Always)'
    yield ''.join(gen_args(args))

def gen_module(cmdtable):
    yield 'use argparse::{Arg, Command, HelpVisibility};\n' \
          'pub fn add_hg_python_commands(c: Command) -> Command{\n' \
          'c'
    for name, rest in cmdtable.iteritems():
        yield '.subcommand(%s)' % ''.join(gen_subcommand(name, *rest))
    yield '\n}'

@command('dump_commands')
def dump_commands(ui, repo, *pats, **opts):
    with open('hg_python_commands.rs', 'w') as f:
        for s in gen_module(commands.table):
            f.write(s)
