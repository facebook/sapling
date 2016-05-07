#!/usr/bin/env python
"""usage: %s DOC ...

where DOC is the name of a document
"""

from __future__ import absolute_import

import os
import sys
import textwrap

# This script is executed during installs and may not have C extensions
# available. Relax C module requirements.
os.environ['HGMODULEPOLICY'] = 'allow'
# import from the live mercurial repo
sys.path.insert(0, "..")
from mercurial import demandimport; demandimport.enable()
from mercurial import (
    commands,
    extensions,
    help,
    minirst,
    ui as uimod,
)
from mercurial.i18n import (
    gettext,
    _,
)

table = commands.table
globalopts = commands.globalopts
helptable = help.helptable
loaddoc = help.loaddoc

def get_desc(docstr):
    if not docstr:
        return "", ""
    # sanitize
    docstr = docstr.strip("\n")
    docstr = docstr.rstrip()
    shortdesc = docstr.splitlines()[0].strip()

    i = docstr.find("\n")
    if i != -1:
        desc = docstr[i + 2:]
    else:
        desc = shortdesc

    desc = textwrap.dedent(desc)

    return (shortdesc, desc)

def get_opts(opts):
    for opt in opts:
        if len(opt) == 5:
            shortopt, longopt, default, desc, optlabel = opt
        else:
            shortopt, longopt, default, desc = opt
            optlabel = _("VALUE")
        allopts = []
        if shortopt:
            allopts.append("-%s" % shortopt)
        if longopt:
            allopts.append("--%s" % longopt)
        if isinstance(default, list):
            allopts[-1] += " <%s[+]>" % optlabel
        elif (default is not None) and not isinstance(default, bool):
            allopts[-1] += " <%s>" % optlabel
        if '\n' in desc:
            # only remove line breaks and indentation
            desc = ' '.join(l.lstrip() for l in desc.split('\n'))
        desc += default and _(" (default: %s)") % default or ""
        yield (", ".join(allopts), desc)

def get_cmd(cmd, cmdtable):
    d = {}
    attr = cmdtable[cmd]
    cmds = cmd.lstrip("^").split("|")

    d['cmd'] = cmds[0]
    d['aliases'] = cmd.split("|")[1:]
    d['desc'] = get_desc(gettext(attr[0].__doc__))
    d['opts'] = list(get_opts(attr[1]))

    s = 'hg ' + cmds[0]
    if len(attr) > 2:
        if not attr[2].startswith('hg'):
            s += ' ' + attr[2]
        else:
            s = attr[2]
    d['synopsis'] = s.strip()

    return d

def showdoc(ui):
    # print options
    ui.write(minirst.section(_("Options")))
    multioccur = False
    for optstr, desc in get_opts(globalopts):
        ui.write("%s\n    %s\n\n" % (optstr, desc))
        if optstr.endswith("[+]>"):
            multioccur = True
    if multioccur:
        ui.write(_("\n[+] marked option can be specified multiple times\n"))
        ui.write("\n")

    # print cmds
    ui.write(minirst.section(_("Commands")))
    commandprinter(ui, table, minirst.subsection)

    # print help topics
    # The config help topic is included in the hgrc.5 man page.
    helpprinter(ui, helptable, minirst.section, exclude=['config'])

    ui.write(minirst.section(_("Extensions")))
    ui.write(_("This section contains help for extensions that are "
               "distributed together with Mercurial. Help for other "
               "extensions is available in the help system."))
    ui.write("\n\n"
             ".. contents::\n"
             "   :class: htmlonly\n"
             "   :local:\n"
             "   :depth: 1\n\n")

    for extensionname in sorted(allextensionnames()):
        mod = extensions.load(ui, extensionname, None)
        ui.write(minirst.subsection(extensionname))
        ui.write("%s\n\n" % gettext(mod.__doc__))
        cmdtable = getattr(mod, 'cmdtable', None)
        if cmdtable:
            ui.write(minirst.subsubsection(_('Commands')))
            commandprinter(ui, cmdtable, minirst.subsubsubsection)

def showtopic(ui, topic):
    extrahelptable = [
        (["common"], '', loaddoc('common')),
        (["hg.1"], '', loaddoc('hg.1')),
        (["hg-ssh.8"], '', loaddoc('hg-ssh.8')),
        (["hgignore.5"], '', loaddoc('hgignore.5')),
        (["hgrc.5"], '', loaddoc('hgrc.5')),
        (["hgignore.5.gendoc"], '', loaddoc('hgignore')),
        (["hgrc.5.gendoc"], '', loaddoc('config')),
    ]
    helpprinter(ui, helptable + extrahelptable, None, include=[topic])

def helpprinter(ui, helptable, sectionfunc, include=[], exclude=[]):
    for names, sec, doc in helptable:
        if exclude and names[0] in exclude:
            continue
        if include and names[0] not in include:
            continue
        for name in names:
            ui.write(".. _%s:\n" % name)
        ui.write("\n")
        if sectionfunc:
            ui.write(sectionfunc(sec))
        if callable(doc):
            doc = doc(ui)
        ui.write(doc)
        ui.write("\n")

def commandprinter(ui, cmdtable, sectionfunc):
    h = {}
    for c, attr in cmdtable.items():
        f = c.split("|")[0]
        f = f.lstrip("^")
        h[f] = c
    cmds = h.keys()
    cmds.sort()

    for f in cmds:
        if f.startswith("debug"):
            continue
        d = get_cmd(h[f], cmdtable)
        ui.write(sectionfunc(d['cmd']))
        # short description
        ui.write(d['desc'][0])
        # synopsis
        ui.write("::\n\n")
        synopsislines = d['synopsis'].splitlines()
        for line in synopsislines:
            # some commands (such as rebase) have a multi-line
            # synopsis
            ui.write("   %s\n" % line)
        ui.write('\n')
        # description
        ui.write("%s\n\n" % d['desc'][1])
        # options
        opt_output = list(d['opts'])
        if opt_output:
            opts_len = max([len(line[0]) for line in opt_output])
            ui.write(_("Options:\n\n"))
            multioccur = False
            for optstr, desc in opt_output:
                if desc:
                    s = "%-*s  %s" % (opts_len, optstr, desc)
                else:
                    s = optstr
                ui.write("%s\n" % s)
                if optstr.endswith("[+]>"):
                    multioccur = True
            if multioccur:
                ui.write(_("\n[+] marked option can be specified"
                           " multiple times\n"))
            ui.write("\n")
        # aliases
        if d['aliases']:
            ui.write(_("    aliases: %s\n\n") % " ".join(d['aliases']))


def allextensionnames():
    return extensions.enabled().keys() + extensions.disabled().keys()

if __name__ == "__main__":
    doc = 'hg.1.gendoc'
    if len(sys.argv) > 1:
        doc = sys.argv[1]

    ui = uimod.ui()
    if doc == 'hg.1.gendoc':
        showdoc(ui)
    else:
        showtopic(ui, sys.argv[1])
