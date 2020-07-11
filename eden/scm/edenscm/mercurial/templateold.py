# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# builtin template definition
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

builtinmapfiles = {
    "bisect": (
        {
            "changeset": "{cset}{lbisect}{branches}{bookmarks}{parents}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("changeset:   {rev}:{node|short}")}\\n',
            "changeset_quiet": "{lshortbisect} {rev}:{node|short}\\n",
            "branch": '{label("log.branch",\n"branch:      {branch}")}\\n',
            "bisectlabel": " bisect.{word('0', bisect)}",
            "fullcset": '{labelcset("changeset:   {rev}:{node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {rev}:{node|formatnode}")}\\n',
            "lbisect": '{label("log.bisect{if(bisect, bisectlabel)}",\n"bisect:      {bisect}\\n")}',
            "lshortbisect": '{label("log.bisect{if(bisect, bisectlabel)}",\n"{bisect|shortbisect}")}',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{rev}:{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{lbisect}{branches}{bookmarks}{parents}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{lbisect}{branches}{bookmarks}{lphase}{parents}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
            "lfile_mods": '{if(file_mods,\nlabel("ui.debug log.files",\n"files:       {file_mods}\\n"))}',
        },
        {},
        [
            (
                "labelcset(expr)",
                'label(separate(" ",\n"log.changeset",\n"changeset.{phase}",\nif(obsolete, "changeset.obsolete")),\nexpr)',
            )
        ],
    ),
    "changelog": (
        {
            "last_file": "{file}:\\n\\t",
            "changeset": "\\t* {files|stringify|fill68|tabindent}{desc|fill68|tabindent|strip}\\n\\t[{node|short}]{branches}\\n\\n",
            "file": "{file}, ",
            "branch": "{branch}, ",
            "file_del": "{file_del}, ",
            "file_add": "{file_add}, ",
            "changeset_verbose": "{date|isodate}  {author|person}  <{author|email}>  ({node|short}{branches})\\n\\n\\t* {file_adds|stringify|fill68|tabindent}{file_dels|stringify|fill68|tabindent}{files|stringify|fill68|tabindent}{desc|fill68|tabindent|strip}\\n\\n",
            "start_branches": " <",
            "header": "{date|shortdate}  {author|person}  <{author|email}>\\n\\n",
            "changeset_quiet": "\\t* {desc|firstline|fill68|tabindent|strip}\\n\\n",
            "last_branch": "{branch}>",
            "last_file_del": "{file_del}: deleted file.\\n* ",
            "header_verbose": "",
            "last_file_add": "{file_add}: new file.\\n* ",
        },
        {},
        [],
    ),
    "compact": (
        {
            "changeset": "{lrev}{bookmarks}{parents}   {lnode}   {ldate}   {luser}\\n  {ldescfirst}\\n\\n",
            "lauthor": '{label("log.user",\n"{author}")}',
            "ldate": '{label("log.date",\n"{date|isodate}")}',
            "parent": "{lrev},",
            "lrev": '{label("log.changeset changeset.{phase}",\n"{rev}")}',
            "bookmark": '{label("log.bookmark",\n"{bookmark},")}',
            "start_parents": ":",
            "changeset_quiet": "{lrev}:{lnode}\\n",
            "changeset_verbose": "{lrev}{parents}   {lnode}   {ldate}   {lauthor}\\n  {ldesc}\\n\\n",
            "ldescfirst": "{label('ui.note log.description',\n'{desc|firstline|strip}')}",
            "ldesc": "{label('ui.note log.description',\n'{desc|strip}')}",
            "last_bookmark": "{bookmark}]",
            "luser": '{label("log.user",\n"{author|user}")}',
            "start_bookmarks": "[",
            "last_parent": "{lrev}",
            "lnode": '{label("log.node",\n"{node|short}")}',
        },
        {},
        [],
    ),
    "default": (
        {
            "changeset": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("changeset:   {rev}:{node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "branch": '{label("log.branch",\n"branch:      {branch}")}\\n',
            "fullcset": '{labelcset("changeset:   {rev}:{node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {rev}:{node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{rev}:{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{branches}{bookmarks}{lphase}{parents}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
            "lfile_mods": '{if(file_mods,\nlabel("ui.debug log.files",\n"files:       {file_mods}\\n"))}',
        },
        {},
        [
            (
                "labelcset(expr)",
                'label(separate(" ",\n"log.changeset",\n"changeset.{phase}",\nif(obsolete, "changeset.obsolete")),\nexpr)',
            )
        ],
    ),
    "phases": (
        {
            "changeset": "{cset}{branches}{bookmarks}{lphase}{parents}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("changeset:   {rev}:{node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "branch": '{label("log.branch",\n"branch:      {branch}")}\\n',
            "fullcset": '{labelcset("changeset:   {rev}:{node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {rev}:{node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{rev}:{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{branches}{bookmarks}{lphase}{parents}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{branches}{bookmarks}{lphase}{parents}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
            "lfile_mods": '{if(file_mods,\nlabel("ui.debug log.files",\n"files:       {file_mods}\\n"))}',
        },
        {},
        [
            (
                "labelcset(expr)",
                'label(separate(" ",\n"log.changeset",\n"changeset.{phase}",\nif(obsolete, "changeset.obsolete")),\nexpr)',
            )
        ],
    ),
    "show": (
        {
            "changeset": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{summary}\\n",
            "cset_namespace": '{ifeq(namespace, "branches", names_branches, names_others)}',
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "names_tags": "{if(names % \"{ifeq(name, 'tip', '', name)}\", \" ({label('log.{colorname}', join(names % \"{ifeq(name, 'tip', '', name)}\", ' '))})\")}",
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "showbookmarks": '{if(active, "*", " ")} {pad(bookmark, longestbookmarklen + 4)}{shortest(node, nodelen)}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("changeset:   {rev}:{node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "branch": '{label("log.branch",\n"branch:      {branch}")}\\n',
            "fullcset": '{labelcset("changeset:   {rev}:{node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "showwork": "{cset_shortnode}{namespaces % cset_namespace} {cset_shortdesc}",
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {rev}:{node|formatnode}")}\\n',
            "cset_shortnode": "{labelcset(shortest(node, nodelen))}",
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "names_branches": '{ifeq(branch, "default", "", " ({label(\'log.{colorname}\', branch)})")}',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{rev}:{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "names_others": "{if(names, \" ({label('log.{colorname}', join(names, ' '))})\")}",
            "changeset_verbose": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "showstack": "{showwork}",
            "changeset_debug": "{fullcset}{branches}{bookmarks}{lphase}{parents}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
            "lfile_mods": '{if(file_mods,\nlabel("ui.debug log.files",\n"files:       {file_mods}\\n"))}',
            "cset_shortdesc": '{label("log.description", desc|firstline)}',
        },
        {},
        [
            (
                "labelcset(expr)",
                'label(separate(" ",\n"log.changeset",\n"changeset.{phase}",\nif(obsolete, "changeset.obsolete")),\nexpr)',
            )
        ],
    ),
    "status": (
        {
            "changeset": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{summary}{lfiles}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": "{if(files,\nlabel('ui.note log.files',\n'files:\\n'))}{lfile_mods}{lfile_adds}{lfile_dels}",
            "lfile_adds": '{file_adds % "{lfile_add}{lfile_src}"}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "lfile_mod": '{label("status.modified", "M {file}\\n")}',
            "lfile_src": '{ifcontains(file, file_copies_switch,\nlabel("status.copied", "  {get(file_copies_switch, file)}\\n"))}',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "lfile_add": '{label("status.added", "A {file}\\n")}',
            "cset": '{labelcset("changeset:   {rev}:{node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "branch": '{label("log.branch",\n"branch:      {branch}")}\\n',
            "fullcset": '{labelcset("changeset:   {rev}:{node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {rev}:{node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": "{file_dels % \"{label('status.removed', 'R {file}\\n')}\"}",
            "lnode": '{label("log.node",\n"{rev}:{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{branches}{bookmarks}{parents}{user}{ldate}{description}{lfiles}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{branches}{bookmarks}{lphase}{parents}{manifest}{user}{ldate}{extras}{description}{lfiles}\\n",
            "lfile_mods": '{file_mods % "{lfile_mod}{lfile_src}"}',
        },
        {},
        [
            (
                "labelcset(expr)",
                'label(separate(" ",\n"log.changeset",\n"changeset.{phase}",\nif(obsolete, "changeset.obsolete")),\nexpr)',
            )
        ],
    ),
    "xml": (
        {
            "changeset": '<logentry revision="{rev}" node="{node}">\\n{branches}{bookmarks}{parents}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n</logentry>\\n',
            "parent": '<parent revision="{rev}" node="{node}" />\\n',
            "extra": '<extra key="{key|xmlescape}">{value|xmlescape}</extra>\\n',
            "bookmark": "<bookmark>{bookmark|xmlescape}</bookmark>\\n",
            "start_file_copies": "<copies>\\n",
            "file_mod": '<path action="M">{file_mod|xmlescape}</path>\\n',
            "file_del": '<path action="R">{file_del|xmlescape}</path>\\n',
            "changeset_verbose": '<logentry revision="{rev}" node="{node}">\\n{branches}{bookmarks}{parents}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n<paths>\\n{file_adds}{file_dels}{file_mods}</paths>\\n{file_copies}</logentry>\\n',
            "file_add": '<path action="A">{file_add|xmlescape}</path>\\n',
            "branch": "<branch>{branch|xmlescape}</branch>\\n",
            "changeset_debug": '<logentry revision="{rev}" node="{node}">\\n{branches}{bookmarks}{parents}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n<paths>\\n{file_adds}{file_dels}{file_mods}</paths>\\n{file_copies}{extras}</logentry>\\n',
            "docfooter": "</log>\\n",
            "end_file_copies": "</copies>\\n",
            "docheader": '<?xml version="1.0"?>\\n<log>\\n',
            "file_copy": '<copy source="{source|xmlescape}">{name|xmlescape}</copy>\\n',
        },
        {},
        [],
    ),
}


logcolumns = (
    "bookmark:    %s\n"
    "branch:      %s\n"
    "changeset:   %s\n"
    "copies:      %s\n"
    "date:        %s\n"
    "extra:       %s=%s\n"
    "files+:      %s\n"
    "files-:      %s\n"
    "files:       %s\n"
    "manifest:    %s\n"
    "obsolete:    %s\n"
    "parent:      %s\n"
    "phase:       %s\n"
    "summary:     %s\n"
    "user:        %s\n"
)


defaulttempl = {
    "parent": "{rev}:{node|formatnode} ",
    "manifest": "{rev}:{node|formatnode}",
    "file_copy": "{name} ({source})",
    "envvar": "{key}={value}",
    "extra": "{key}={value|stringescape}",
    "nodechange": "{oldnode|short} -> "
    '{join(newnodes % "{newnode|short}", ", ")|nonempty}\n',
}

# filecopy is preserved for compatibility reasons
defaulttempl["filecopy"] = defaulttempl["file_copy"]
