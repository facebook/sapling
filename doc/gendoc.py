import sys, textwrap
# import from the live mercurial repo
sys.path.insert(0, "..")
from mercurial import demandimport; demandimport.enable()
from mercurial.commands import table, globalopts
from mercurial.i18n import gettext as _
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
        desc = docstr[i+2:]
    else:
        desc = "    %s" % shortdesc
    return (shortdesc, desc)

def get_opts(opts):
    for shortopt, longopt, default, desc in opts:
        allopts = []
        if shortopt:
            allopts.append("-%s" % shortopt)
        if longopt:
            allopts.append("--%s" % longopt)
        desc += default and _(" (default: %s)") % default or ""
        yield(", ".join(allopts), desc)

def get_cmd(cmd):
    d = {}
    attr = table[cmd]
    cmds = cmd.lstrip("^").split("|")

    d['synopsis'] = attr[2]
    d['cmd'] = cmds[0]
    d['aliases'] = cmd.split("|")[1:]
    d['desc'] = get_desc(attr[0].__doc__)
    d['opts'] = list(get_opts(attr[1]))
    return d


def show_doc(ui):
    def bold(s, text=""):
        ui.write("%s\n%s\n%s\n" % (s, "="*len(s), text))
    def underlined(s, text=""):
        ui.write("%s\n%s\n%s\n" % (s, "-"*len(s), text))

    # print options
    underlined(_("OPTIONS"))
    for optstr, desc in get_opts(globalopts):
        ui.write("%s::\n    %s\n\n" % (optstr, desc))

    # print cmds
    underlined(_("COMMANDS"))
    h = {}
    for c, attr in table.items():
        f = c.split("|")[0]
        f = f.lstrip("^")
        h[f] = c
    cmds = h.keys()
    cmds.sort()

    for f in cmds:
        if f.startswith("debug"): continue
        d = get_cmd(h[f])
        # synopsis
        ui.write("[[%s]]\n" % d['cmd'])
        ui.write("%s::\n" % d['synopsis'].replace("hg ","", 1))
        # description
        ui.write("%s\n\n" % d['desc'][1])
        # options
        opt_output = list(d['opts'])
        if opt_output:
            opts_len = max([len(line[0]) for line in opt_output])
            ui.write(_("    options:\n"))
            for optstr, desc in opt_output:
                if desc:
                    s = "%-*s  %s" % (opts_len, optstr, desc)
                else:
                    s = optstr
                s = textwrap.fill(s, initial_indent=4 * " ",
                                  subsequent_indent=(6 + opts_len) * " ")
                ui.write("%s\n" % s)
            ui.write("\n")
        # aliases
        if d['aliases']:
            ui.write(_("    aliases: %s\n\n") % " ".join(d['aliases']))

    # print topics
    for t, doc in helptable:
        l = t.split("|")
        section = l[-1]
        underlined(_(section).upper())
        if callable(doc):
            doc = doc()
        ui.write(_(doc))
        ui.write("\n")

if __name__ == "__main__":
    show_doc(sys.stdout)
