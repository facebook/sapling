import os, sys
# import from the live mercurial repo
sys.path.insert(0, "..")
# fall back to pure modules if required C extensions are not available
sys.path.append(os.path.join('..', 'mercurial', 'pure'))
from mercurial import demandimport; demandimport.enable()
from mercurial import encoding
from mercurial.commands import table, globalopts
from mercurial.i18n import _
from mercurial.help import helptable

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
        desc = "    %s" % shortdesc
    return (shortdesc, desc)

def get_opts(opts):
    for opt in opts:
        if len(opt) == 5:
            shortopt, longopt, default, desc, optlabel = opt
        else:
            shortopt, longopt, default, desc = opt
        allopts = []
        if shortopt:
            allopts.append("-%s" % shortopt)
        if longopt:
            allopts.append("--%s" % longopt)
        desc += default and _(" (default: %s)") % default or ""
        yield(", ".join(allopts), desc)

def get_cmd(cmd, cmdtable):
    d = {}
    attr = cmdtable[cmd]
    cmds = cmd.lstrip("^").split("|")

    d['cmd'] = cmds[0]
    d['aliases'] = cmd.split("|")[1:]
    d['desc'] = get_desc(attr[0].__doc__)
    d['opts'] = list(get_opts(attr[1]))

    s = 'hg ' + cmds[0]
    if len(attr) > 2:
        if not attr[2].startswith('hg'):
            s += ' ' + attr[2]
        else:
            s = attr[2]
    d['synopsis'] = s.strip()

    return d

def section(ui, s):
    ui.write("%s\n%s\n\n" % (s, "-" * encoding.colwidth(s)))

def subsection(ui, s):
    ui.write("%s\n%s\n\n" % (s, '"' * encoding.colwidth(s)))


def show_doc(ui):
    # print options
    section(ui, _("Options"))
    for optstr, desc in get_opts(globalopts):
        ui.write("%s\n    %s\n\n" % (optstr, desc))

    # print cmds
    section(ui, _("Commands"))
    commandprinter(ui, table)

    # print topics
    for names, sec, doc in helptable:
        for name in names:
            ui.write(".. _%s:\n" % name)
        ui.write("\n")
        section(ui, sec)
        if hasattr(doc, '__call__'):
            doc = doc()
        ui.write(doc)
        ui.write("\n")

def commandprinter(ui, cmdtable):
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
        subsection(ui, d['cmd'])
        # synopsis
        ui.write("``%s``\n" % d['synopsis'].replace("hg ","", 1))
        # description
        ui.write("%s\n\n" % d['desc'][1])
        # options
        opt_output = list(d['opts'])
        if opt_output:
            opts_len = max([len(line[0]) for line in opt_output])
            ui.write(_("    options:\n\n"))
            for optstr, desc in opt_output:
                if desc:
                    s = "%-*s  %s" % (opts_len, optstr, desc)
                else:
                    s = optstr
                ui.write("    %s\n" % s)
            ui.write("\n")
        # aliases
        if d['aliases']:
            ui.write(_("    aliases: %s\n\n") % " ".join(d['aliases']))


if __name__ == "__main__":
    show_doc(sys.stdout)
