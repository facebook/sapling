# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# help.py - help data for mercurial
#
# Copyright 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import itertools
import textwrap
from typing import List, Set

from bindings import cliparser

from . import (
    cmdutil,
    encoding,
    error,
    extensions,
    filemerge,
    fileset,
    helptext,
    identity,
    minirst,
    pycompat,
    revset,
    templatefilters,
    templatekw,
    templater,
    util,
)
from .i18n import _, gettext


_exclkeywords: Set[str] = {
    "(ADVANCED)",
    "(DEPRECATED)",
    "(EXPERIMENTAL)",
    "(HIDDEN)",
    # i18n: "(ADVANCED)" is a keyword, must be translated consistently
    _("(ADVANCED)"),
    # i18n: "(DEPRECATED)" is a keyword, must be translated consistently
    _("(DEPRECATED)"),
    # i18n: "(EXPERIMENTAL)" is a keyword, must be translated consistently
    _("(EXPERIMENTAL)"),
}


def listexts(header, exts, indent: int = 1, showdeprecated: bool = False) -> List[str]:
    """return a text listing of the given extensions"""
    rst = []
    if exts:
        for name, desc in sorted(pycompat.iteritems(exts)):
            if not showdeprecated and any(w in desc for w in _exclkeywords):
                continue
            rst.append("%s:%s: %s\n" % (" " * indent, name, desc))
    if rst:
        rst.insert(0, "\n%s\n\n" % header)
    return rst


def extshelp(ui) -> str:
    rst = loaddoc("extensions")(ui).splitlines(True)
    rst.extend(
        listexts(_("Enabled extensions:"), extensions.enabled(), showdeprecated=True)
    )
    rst.extend(listexts(_("Disabled extensions:"), extensions.disabled()))
    doc = "".join(rst)
    return doc


def optrst(header: str, options, verbose) -> str:
    data = []
    multioccur = False
    for option in options:
        if len(option) == 5:
            shortopt, longopt, default, desc, optlabel = option
        else:
            shortopt, longopt, default, desc = option
            optlabel = _("VALUE")  # default label

        if not verbose and any(w in desc for w in _exclkeywords):
            continue

        so = ""
        if shortopt:
            so = "-" + shortopt
        lo = "--" + longopt
        if default:
            # default is of unknown type, and in Python 2 we abused
            # the %s-shows-repr property to handle integers etc. To
            # match that behavior on Python 3, we do str(default) and
            # then convert it to bytes.
            desc += _(" (default: %s)") % pycompat.bytestr(default)

        if isinstance(default, list):
            lo += " %s [+]" % optlabel
            multioccur = True
        elif (default is not None) and not isinstance(default, bool):
            lo += " %s" % optlabel

        data.append((so, lo, desc))

    if not data:
        return ""

    if multioccur:
        header += _(" ([+] can be repeated)")

    rst = ["\n%s:\n\n" % header]
    rst.extend(minirst.maketable(data, 1))

    return "".join(rst)


def indicateomitted(rst, omitted, notomitted=None) -> None:
    rst.append("\n\n.. container:: omitted\n\n    %s\n\n" % omitted)
    if notomitted:
        rst.append("\n\n.. container:: notomitted\n\n    %s\n\n" % notomitted)


def filtercmd(ui, cmd, kw, doc) -> bool:
    if not ui.debugflag and cmd.startswith("debug") and kw != "debug":
        return True
    if not ui.verbose and doc and any(w in doc for w in _exclkeywords):
        return True
    return False


def loaddoc(topic, subdir=None):
    """Return a delayed loader for help/topic.txt."""

    def loader(ui):
        doc = gettext(getattr(helptext, topic))
        for rewriter in helphooks.get(topic, []):
            doc = rewriter(ui, topic, doc)
        return doc

    return loader


helptable = sorted(
    [
        (["bundlespec"], _("Bundle File Formats"), loaddoc("bundlespec")),
        (["color"], _("Colorizing Outputs"), loaddoc("color")),
        (["config", "hgrc"], _("Configuration Files"), loaddoc("config")),
        (["dates"], _("Date Formats"), loaddoc("dates")),
        (["flags"], _("Command-line flags"), loaddoc("flags")),
        (["patterns"], _("Specifying Files by File Name Pattern"), loaddoc("patterns")),
        (["environment", "env"], _("Environment Variables"), loaddoc("environment")),
        (
            ["revisions", "revs", "revsets", "revset", "multirevs", "mrevs"],
            _("Specifying Commits"),
            loaddoc("revisions"),
        ),
        (
            ["filesets", "fileset"],
            _("Specifying Files by their Characteristics"),
            loaddoc("filesets"),
        ),
        (["diffs"], _("Diff Formats"), loaddoc("diffs")),
        (
            ["merge-tools", "mergetools", "mergetool"],
            _("Merge Tools"),
            loaddoc("merge-tools"),
        ),
        (
            ["templating", "templates", "template", "style"],
            _("Customizing Output with Templates"),
            loaddoc("templates"),
        ),
        (["urls"], _("URL Paths"), loaddoc("urls")),
        (["extensions"], _("Using Additional Features"), extshelp),
        (["glossary"], _("Common Terms"), loaddoc("glossary")),
        (["phases"], _("Working with Phases"), loaddoc("phases")),
        (
            ["scripting"],
            _("Using @Product@ from scripts and automation"),
            loaddoc("scripting"),
        ),
        (["pager"], _("Pager Support"), loaddoc("pager")),
    ]
)

# Maps topics with sub-topics to a list of their sub-topics.
subtopics = {}

# Map topics to lists of callable taking the current topic help and
# returning the updated version
helphooks = {}


def addtopichook(topic, rewriter) -> None:
    helphooks.setdefault(topic, []).append(rewriter)


def makeitemsdoc(ui, topic, doc, marker, items, dedent: bool = False):
    """Extract docstring from the items key to function mapping, build a
    single documentation block and use it to overwrite the marker in doc.
    """
    entries = []
    seen = set()
    for name in sorted(items):
        # Hide private functions like "_all()".
        if name.startswith("_"):
            continue
        if items[name] in seen:
            continue
        seen.add(items[name])
        text = (pycompat.getdoc(items[name]) or "").rstrip()
        if not text or not ui.verbose and any(w in text for w in _exclkeywords):
            continue
        text = gettext(text)
        if dedent:
            text = textwrap.dedent(text)
        lines = text.splitlines()
        doclines = [(lines[0])]
        for l in lines[1:]:
            # Stop once we find some Python doctest
            if l.strip().startswith(">>>"):
                break
            if dedent:
                doclines.append(l.rstrip())
            else:
                doclines.append("  " + l.strip())
        entries.append("\n".join(doclines))
    entries = "\n\n".join(entries)
    return doc.replace(marker, entries)


def makesubcmdlist(cmd, categories, subcommands, verbose, quiet) -> List[str]:
    subcommandindex = {}
    for name, entry in subcommands.items():
        for alias in cmdutil.parsealiases(name):
            subcommandindex[alias] = name

    def getsubcommandrst(name, alias=None):
        entry = subcommands[name]
        doc = pycompat.getdoc(entry[0]) or ""
        doc = gettext(doc)
        if not verbose and doc and any(w in doc for w in _exclkeywords):
            return []
        if doc:
            doc = doc.splitlines()[0].rstrip()
        if not doc:
            doc = _("(no help text available)")
        aliases = cmdutil.parsealiases(name)
        if verbose:
            name = ", ".join(aliases)
            if len(entry) > 2:
                name = "%s %s" % (name, entry[2])
        else:
            name = alias or aliases[0]
        return [" :%s: %s\n" % (name, doc)]

    rst = []
    seen = set()
    if categories:
        for category, aliases in categories:
            categoryrst = []
            for alias in aliases:
                name = subcommandindex.get(alias)
                if name:
                    seen.add(name)
                    categoryrst.extend(getsubcommandrst(name, alias))
            if categoryrst:
                rst.append("\n%s:\n\n" % category)
                rst.extend(categoryrst)

    otherrst = []
    for name in sorted(subcommands.keys()):
        if name not in seen:
            otherrst.extend(getsubcommandrst(name))
    if otherrst:
        rst.append("\n%s:\n\n" % (_("Other Subcommands") if seen else _("Subcommands")))
        rst.extend(otherrst)

    if not quiet:
        rst.append(
            _("\n(use '@prog@ help %s SUBCOMMAND' to show complete subcommand help)\n")
            % cmd
        )
    return rst


def addtopicsymbols(topic, marker, symbols, dedent: bool = False) -> None:
    def add(ui, topic, doc):
        return makeitemsdoc(ui, topic, doc, marker, symbols, dedent=dedent)

    addtopichook(topic, add)


addtopicsymbols(
    "bundlespec", ".. bundlecompressionmarker", util.bundlecompressiontopics()
)
addtopicsymbols("filesets", ".. predicatesmarker", fileset.symbols)
addtopicsymbols("merge-tools", ".. internaltoolsmarker", filemerge.internalsdoc)
addtopicsymbols("revisions", ".. predicatesmarker", revset.symbols)
addtopicsymbols("templates", ".. keywordsmarker", templatekw.keywords)
addtopicsymbols("templates", ".. filtersmarker", templatefilters.filters)
addtopicsymbols("templates", ".. functionsmarker", templater.funcs)

helphomecommands = [
    ("Get the latest commits from the server", ["pull"]),
    ("View commits", ["ssl", "show", "diff"]),
    ("Check out a commit", ["goto"]),
    (
        "Work with your checkout",
        ["status", "add", "remove", "forget", "revert", "purge", "shelve"],
    ),
    ("Commit changes and modify commits", ["commit", "amend", "metaedit"]),
    ("Rearrange commits", ["rebase", "graft", "hide", "unhide"]),
    (
        "Work with stacks of commits",
        ["previous", "next", "split", "fold", "histedit", "absorb"],
    ),
    ("Undo changes", ["uncommit", "unamend", "undo", "redo"]),
    ("Other commands", ["config", "doctor", "grep", "journal", "rage", "web"]),
]

helphometopics = {"revisions", "filesets", "glossary", "patterns", "templating"}


class _helpdispatch(object):
    def __init__(
        self, ui, commands, unknowncmd=False, full=False, subtopic=None, **opts
    ):
        self.ui = ui
        self.commands = commands
        self.subtopic = subtopic
        self.unknowncmd = unknowncmd
        self.full = full
        self.opts = opts

        self.commandshelptable = util.sortdict()
        for cmd, entry in pycompat.iteritems(self.commands.table):
            self.commandshelptable[cmd] = (
                getattr(entry[0], "__rusthelp__", None) or entry
            )

        self.commandindex = {}
        for name, cmd in pycompat.iteritems(self.commandshelptable):
            for n in name.lstrip("^").split("|"):
                self.commandindex[n] = cmd

    def dispatch(self, name):
        queries = []
        if self.unknowncmd:
            queries += [self.helpextcmd]
        if self.opts.get("extension"):
            queries += [self.helpext]
        if self.opts.get("command"):
            queries += [self.helpcmd]
        if not queries:
            queries = (self.helptopic, self.helpcmd, self.helpext, self.helpextcmd)
        for f in queries:
            try:
                return f(name, self.subtopic)
            except error.UnknownCommand:
                pass
        else:
            if self.unknowncmd:
                raise error.UnknownCommand(name)
            else:
                msg = _("no such help topic: %s") % name
                hint = _("try '@prog@ help --keyword %s'") % name
                raise error.Abort(msg, hint=hint)

    def helpcmd(self, name, subtopic=None):
        ui = self.ui
        try:
            # Try to expand 'name' as an alias
            resolvedargs = cliparser.expandargs(ui._rcfg, name.split())[0]
            if name == "debug":
                raise cliparser.AmbiguousCommand()
        except cliparser.AmbiguousCommand:
            select = lambda c: c.lstrip("^").partition("|")[0].startswith(name)
            rst = self.helplist(name, select)
            return rst
        except cliparser.MalformedAlias as ex:
            raise error.Abort(ex.args[0])
        if " ".join(resolvedargs) != name:
            self.ui.write(_("alias for: %s\n\n") % " ".join(resolvedargs))
            # Try to print ":doc" from alias configs
            doc = ui.config("alias", "%s:doc" % name)
            if doc:
                self.ui.write("%s\n\n" % doc)
            # Continue with the resolved (non-alias) name
            name = " ".join(resolvedargs)

        try:
            cmd, args, aliases, entry, _level = cmdutil.findsubcmd(
                name.split(), self.commandshelptable, partial=True
            )
        except error.AmbiguousCommand as inst:
            # py3k fix: except vars can't be used outside the scope of the
            # except block, nor can be used inside a lambda. python issue4617
            prefix = inst.args[0]
            select = lambda c: c.lstrip("^").partition("|")[0].startswith(prefix)
            rst = self.helplist(name, select)
            return rst
        except error.UnknownSubcommand as inst:
            cmd, subcmd = inst.args[:2]
            msg = _("'%s' has no such subcommand: %s") % (cmd, subcmd)
            hint = _("run '@prog@ help %s' to see available subcommands") % cmd
            raise error.Abort(msg, hint=hint)

        rst = []

        # check if it's an invalid alias and display its error if it is
        if getattr(entry[0], "badalias", None):
            rst.append(entry[0].badalias + "\n")
            if entry[0].unknowncmd:
                try:
                    rst.extend(self.helpextcmd(entry[0].cmdname))
                except error.UnknownCommand:
                    pass
            return rst

        # synopsis
        if len(entry) > 2:
            if entry[2].startswith("hg"):
                rst.append("%s\n" % entry[2])
            else:
                rst.append("%s %s %s\n" % (identity.default().cliname(), cmd, entry[2]))
        else:
            rst.append("%s %s\n" % (identity.default().cliname(), cmd))
        # aliases
        # try to simplify aliases, ex. compress ['ab', 'abc', 'abcd', 'abcde']
        # to ['ab', 'abcde']
        slimaliases = []
        sortedaliases = sorted(aliases)
        for i, alias in enumerate(sortedaliases):
            if slimaliases and i + 1 < len(aliases):
                nextalias = sortedaliases[i + 1]
                if nextalias.startswith(alias) and alias.startswith(slimaliases[-1]):
                    # Skip this alias
                    continue
            slimaliases.append(alias)
        slimaliases = set(slimaliases)

        if self.full and not self.ui.quiet and len(slimaliases) > 1:
            rst.append(
                _("\naliases: %s\n")
                % ", ".join(a for a in aliases[1:] if a in slimaliases)
            )
        rst.append("\n")

        # description
        doc = gettext(pycompat.getdoc(entry[0]))

        if not doc:
            doc = _("(no help text available)")
        if util.safehasattr(entry[0], "definition"):  # aliased command
            aliasdoc = ""
            if util.safehasattr(entry[0], "aliasdoc") and entry[0].aliasdoc is not None:
                lines = entry[0].aliasdoc.splitlines()
                if lines:
                    aliasdoc = (
                        "\n".join(templater.unquotestring(l) for l in lines) + "\n\n"
                    )
            source = entry[0].source
            if entry[0].definition.startswith("!"):  # shell alias
                doc = _("%sshell alias for::\n\n    %s\n\ndefined by: %s\n") % (
                    aliasdoc,
                    entry[0].definition[1:],
                    source,
                )
            else:
                doc = _("%salias for: @prog@ %s\n\n%s\n\ndefined by: %s\n") % (
                    aliasdoc,
                    entry[0].definition,
                    doc,
                    source,
                )
        doc = doc.splitlines(True)
        if self.ui.quiet or not self.full:
            rst.append(doc[0])
        else:
            rst.extend(doc)
        rst.append("\n")

        # check if this command shadows a non-trivial (multi-line)
        # extension help text
        try:
            mod = extensions.find(name)
            doc = gettext(pycompat.getdoc(mod)) or ""
            if "\n" in doc.strip():
                msg = _(
                    "(use '@prog@ help -e %s' to show help for the %s extension)"
                ) % (
                    name,
                    name,
                )
                rst.append("\n%s\n" % msg)
        except KeyError:
            pass

        # options
        if not self.ui.quiet and entry[1]:
            rst.append(optrst(_("Options"), entry[1], self.ui.verbose))

        if self.ui.verbose:
            rst.append(
                optrst(_("Global options"), self.commands.globalopts, self.ui.verbose)
            )

        # subcommands
        if util.safehasattr(entry[0], "subcommands") and entry[0].subcommands:
            rst.extend(
                makesubcmdlist(
                    cmd,
                    entry[0].subcommandcategories,
                    entry[0].subcommands,
                    self.ui.verbose,
                    self.ui.quiet,
                )
            )

        if not self.ui.verbose:
            if not self.full:
                rst.append(_("\n(use '@prog@ %s -h' to show more help)\n") % name)
            elif not self.ui.quiet:
                rst.append(
                    _("\n(some details hidden, use --verbose to show complete help)")
                )

        return rst

    def _helpcmddoc(self, cmd, doc):
        if util.safehasattr(cmd, "aliasdoc") and cmd.aliasdoc is not None:
            return gettext(templater.unquotestring(cmd.aliasdoc.splitlines()[0]))
        doc = gettext(doc)
        if doc:
            doc = doc.splitlines()[0].rstrip()
        if not doc:
            doc = _("(no help text available)")
        return doc

    def _helpcmditem(self, name):
        cmd = self.commandindex.get(name)
        if cmd is None:
            return None
        doc = self._helpcmddoc(cmd[0], pycompat.getdoc(cmd[0]))
        return " :%s: %s\n" % (name, doc)

    def helplist(self, name, select=None, **opts):
        h = {}
        cmds = {}
        for c, e in pycompat.iteritems(self.commandshelptable):
            if select and not select(c):
                continue
            f = c.lstrip("^").partition("|")[0]
            doc = pycompat.getdoc(e[0])
            if filtercmd(self.ui, f, name, doc):
                continue
            h[f] = self._helpcmddoc(e[0], doc)
            cmds[f] = c.lstrip("^")

        rst = []
        if not h:
            if not self.ui.quiet:
                rst.append(_("no commands defined\n"))
            return rst

        if not self.ui.quiet:
            if name == "debug":
                header = _("Debug commands (internal and unsupported):\n\n")
            else:
                header = _("Commands:\n\n")
            rst.append(header)

        fns = sorted(h)
        for f in fns:
            if self.ui.verbose:
                commacmds = cmds[f].replace("|", ", ")
                rst.append(" :%s: %s\n" % (commacmds, h[f]))
            else:
                rst.append(" :%s: %s\n" % (f, h[f]))

        return rst

    def helphome(self):
        rst = [
            _("@LongProduct@\n"),
            "\n",
            "@prog@ COMMAND [OPTIONS]\n",
            "\n",
            "These are some common @Product@ commands.  Use '@prog@ help commands' to list all "
            "commands, and '@prog@ help COMMAND' to get help on a specific command.\n",
            "\n",
        ]

        for desc, commands in helphomecommands:

            sectionrst = []
            for command in commands:
                cmdrst = self._helpcmditem(command)
                if cmdrst:
                    sectionrst.append(cmdrst)

            if sectionrst:
                rst.append(desc + ":\n\n")
                rst.extend(sectionrst)
                rst.append("\n")

        topics = []
        for names, header, doc in helptable:
            if names[0] in helphometopics:
                topics.append((names[0], header))
        if topics:
            rst.append(_("\nAdditional help topics:\n\n"))
            for t, desc in topics:
                rst.append(" :%s: %s\n" % (t, desc.lower()))

        localhelp = self.ui.config("help", "localhelp")
        if localhelp:
            rst.append("\n")
            rst.append(localhelp)

        return rst

    def helptopic(self, name, subtopic=None):
        # Look for sub-topic entry first.
        header, doc = None, None
        if subtopic and name in subtopics:
            for names, header, doc in subtopics[name]:
                if subtopic in names:
                    break

        if not header:
            for names, header, doc in helptable:
                if name in names:
                    break
            else:
                raise error.UnknownCommand(name)

        rst = [minirst.section(header)]

        # description
        if not doc:
            rst.append("    %s\n" % _("(no help text available)"))
        if callable(doc):
            rst += ["    %s\n" % l for l in doc(self.ui).splitlines()]

        if not self.ui.verbose:
            omitted = _("(some details hidden, use --verbose to show complete help)")
            indicateomitted(rst, omitted)

        try:
            cmdutil.findcmd(name, self.commandshelptable)
            rst.append(
                _("\nuse '@prog@ help -c %s' to see help for the %s command\n")
                % (name, name)
            )
        except error.UnknownCommand:
            pass
        return rst

    def helpext(self, name, subtopic=None):
        try:
            mod = extensions.find(name)
            doc = gettext(pycompat.getdoc(mod)) or _("no help text available")
        except KeyError:
            mod = None
            doc = extensions.disabledext(name)
            if not doc:
                raise error.UnknownCommand(name)

        if "\n" not in doc:
            head, tail = doc, ""
        else:
            head, tail = doc.split("\n", 1)
        rst = [_("%s extension - %s\n\n") % (name.rpartition(".")[-1], head)]
        if tail:
            rst.extend(tail.splitlines(True))
            rst.append("\n")

        if not self.ui.verbose:
            omitted = _("(some details hidden, use --verbose to show complete help)")
            indicateomitted(rst, omitted)

        if mod:
            try:
                ct = mod.cmdtable
            except AttributeError:
                ct = {}
            rst.extend(self.helplist(name, ct.__contains__))
        else:
            rst.append(
                _(
                    "(use '@prog@ help extensions' for information on enabling"
                    " extensions)\n"
                )
            )
        return rst

    def helpextcmd(self, name, subtopic=None):
        cmd, ext, mod = extensions.disabledcmd(self.ui, name)
        doc = gettext(pycompat.getdoc(mod))
        if doc is None:
            doc = _("(no help text available)")
        else:
            doc = doc.splitlines()[0]

        rst = listexts(
            _("'%s' is provided by the following extension:") % cmd,
            {ext: doc},
            indent=4,
            showdeprecated=True,
        )
        rst.append("\n")
        rst.append(
            _("(use '@prog@ help extensions' for information on enabling extensions)\n")
        )
        return rst

    def topicmatch(self, kw):
        """Return help topics matching kw.

        Returns {'section': [(name, summary), ...], ...} where section is
        one of topics, commands, extensions, or extensioncommands.
        """
        kw = encoding.lower(kw)

        def lowercontains(container):
            return kw in encoding.lower(container)  # translated in helptable

        results = {
            "topics": [],
            "commands": [],
            "extensions": [],
            "extensioncommands": [],
        }
        for names, header, doc in helptable:
            # Old extensions may use a str as doc.
            if (
                sum(map(lowercontains, names))
                or lowercontains(header)
                or (callable(doc) and lowercontains(doc(self.ui)))
            ):
                results["topics"].append((names[0], header))
        for cmd, entry in pycompat.iteritems(self.commandshelptable):
            if len(entry) == 3:
                summary = entry[2]
            else:
                summary = ""
            # translate docs *before* searching there
            docs = _(pycompat.getdoc(entry[0])) or ""
            if kw in cmd or lowercontains(summary) or lowercontains(docs):
                doclines = docs.splitlines()
                if doclines:
                    summary = doclines[0]
                cmdname = cmd.partition("|")[0].lstrip("^")
                if filtercmd(self.ui, cmdname, kw, docs):
                    continue
                results["commands"].append((cmdname, summary))
        for name, docs in itertools.chain(
            pycompat.iteritems(extensions.enabled(False)),
            pycompat.iteritems(extensions.disabled()),
        ):
            if not docs:
                continue
            name = name.rpartition(".")[-1]
            if lowercontains(name) or lowercontains(docs):
                # extension docs are already translated
                results["extensions"].append((name, docs.splitlines()[0]))
            try:
                mod = extensions.load(self.ui, name, "")
            except ImportError:
                # debug message would be printed in extensions.load()
                continue
            for cmd, entry in pycompat.iteritems(getattr(mod, "cmdtable", {})):
                if kw in cmd or (len(entry) > 2 and lowercontains(entry[2])):
                    cmdname = cmd.partition("|")[0].lstrip("^")
                    cmddoc = pycompat.getdoc(entry[0])
                    if cmddoc:
                        cmddoc = gettext(cmddoc).splitlines()[0]
                    else:
                        cmddoc = _("(no help text available)")
                    if filtercmd(self.ui, cmdname, kw, cmddoc):
                        continue
                    results["extensioncommands"].append((cmdname, cmddoc))
        return results


def help_(
    ui,
    commands,
    name,
    unknowncmd: bool = False,
    full: bool = True,
    subtopic=None,
    **opts,
) -> str:
    """
    Generate the help for 'name' as unformatted restructured text. If
    'name' is None, describe the commands available.
    """
    dispatch = _helpdispatch(ui, commands, unknowncmd, full, subtopic, **opts)

    rst = []
    kw = opts.get("keyword")
    if kw or name is None and any(opts[o] for o in opts):
        matches = dispatch.topicmatch(name or "")
        helpareas = []
        if opts.get("extension"):
            helpareas += [("extensions", _("Extensions"))]
        if opts.get("command"):
            helpareas += [("commands", _("Commands"))]
        if not helpareas:
            helpareas = [
                ("topics", _("Topics")),
                ("commands", _("Commands")),
                ("extensions", _("Extensions")),
                ("extensioncommands", _("Extension Commands")),
            ]
        for t, title in helpareas:
            if matches[t]:
                rst.append("%s:\n\n" % title)
                rst.extend(minirst.maketable(sorted(matches[t]), 1))
                rst.append("\n")
        if not rst:
            msg = _("no matches")
            hint = _("try '@prog@ help' for a list of topics")
            raise error.Abort(msg, hint=hint)
    elif name == "commands":
        if not ui.quiet:
            rst = [_("@LongProduct@\n"), "\n"]
        rst.extend(dispatch.helplist(None, None, **opts))
    elif name:
        rst = dispatch.dispatch(name)
    else:
        rst = dispatch.helphome()

    return "".join(rst)


def formattedhelp(
    ui, commands, name, keep=None, unknowncmd: bool = False, full: bool = True, **opts
):
    """get help for a given topic (as a dotted name) as rendered rst

    Either returns the rendered help text or raises an exception.
    """
    if keep is None:
        keep = []
    else:
        keep = list(keep)  # make a copy so we can mutate this later
    fullname = name
    section = None
    subtopic = None
    if name and "." in name:
        name, remaining = name.split(".", 1)
        remaining = encoding.lower(remaining)
        if "." in remaining:
            subtopic, section = remaining.split(".", 1)
        else:
            if name in subtopics:
                subtopic = remaining
            else:
                section = remaining
    textwidth = ui.configint("ui", "textwidth")
    termwidth = ui.termwidth() - 2
    if textwidth <= 0 or termwidth < textwidth:
        textwidth = termwidth
    text = help_(
        ui, commands, name, subtopic=subtopic, unknowncmd=unknowncmd, full=full, **opts
    )

    formatted, pruned = minirst.format(text, textwidth, keep=keep, section=section)

    # We could have been given a weird ".foo" section without a name
    # to look for, or we could have simply failed to found "foo.bar"
    # because bar isn't a section of foo
    if section and not (formatted and name):
        raise error.Abort(_("help section not found: %s") % fullname)

    if "verbose" in pruned:
        keep.append("omitted")
    else:
        keep.append("notomitted")
    formatted, pruned = minirst.format(text, textwidth, keep=keep, section=section)
    return formatted
