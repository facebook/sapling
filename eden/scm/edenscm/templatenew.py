# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# builtin template definition (new)
#
# Copyright 2005, 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

builtinmapfiles = {
    "bisect": (
        {
            "changeset": "{cset}{lbisect}{bookmarks}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("commit:      {node|short}")}\\n',
            "changeset_quiet": "{lshortbisect} {node|short}\\n",
            "bisectlabel": " bisect.{word('0', bisect)}",
            "fullcset": '{labelcset("commit:      {node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {node|formatnode}")}\\n',
            "lbisect": '{label("log.bisect{if(bisect, bisectlabel)}",\n"bisect:      {bisect}\\n")}',
            "lshortbisect": '{label("log.bisect{if(bisect, bisectlabel)}",\n"{bisect|shortbisect}")}',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{lbisect}{bookmarks}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{lbisect}{bookmarks}{lphase}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
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
            "changeset": "\\t* {files|stringify|fill68|tabindent}{desc|fill68|tabindent|strip}\\n\\t[{node|short}]\\n\\n",
            "file": "{file}, ",
            "file_del": "{file_del}, ",
            "file_add": "{file_add}, ",
            "changeset_verbose": "{date|isodate}  {author|person}  <{author|email}>  ({node|short})\\n\\n\\t* {file_adds|stringify|fill68|tabindent}{file_dels|stringify|fill68|tabindent}{files|stringify|fill68|tabindent}{desc|fill68|tabindent|strip}\\n\\n",
            "header": "{date|shortdate}  {author|person}  <{author|email}>\\n\\n",
            "changeset_quiet": "\\t* {desc|firstline|fill68|tabindent|strip}\\n\\n",
            "last_file_del": "{file_del}: deleted file.\\n* ",
            "header_verbose": "",
            "last_file_add": "{file_add}: new file.\\n* ",
        },
        {},
        [],
    ),
    "compact": (
        {
            "changeset": "{bookmarks}   {lnode}   {ldate}   {luser}\\n  {ldescfirst}\\n\\n",
            "lauthor": '{label("log.user",\n"{author}")}',
            "ldate": '{label("log.date",\n"{date|isodate}")}',
            "parent": "{lnode},",
            "bookmark": '{label("log.bookmark",\n"{bookmark},")}',
            "start_parents": ":",
            "changeset_quiet": "{{lnode}\\n",
            "changeset_verbose": "{lnode}   {ldate}   {lauthor}\\n  {ldesc}\\n\\n",
            "ldescfirst": "{label('ui.note log.description',\n'{desc|firstline|strip}')}",
            "ldesc": "{label('ui.note log.description',\n'{desc|strip}')}",
            "last_bookmark": "{bookmark}]",
            "luser": '{label("log.user",\n"{author|user}")}',
            "start_bookmarks": "[",
            "last_parent": "{lnode}",
            "lnode": '{label("log.node",\n"{node|short}")}',
        },
        {},
        [],
    ),
    "default": (
        {
            "changeset": "{cset}{bookmarks}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("commit:      {node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "fullcset": '{labelcset("commit:      {node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{bookmarks}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{bookmarks}{lphase}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
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
    # Based on facebook.style
    "sl_default": (
        {
            "changeset": "{cset}{branches}{onelinebookmarks}{onelinetags}{user}{ldate}{summary}\n",
            "changeset_quiet": "{date|localdate|isodate}  {shorthash} {shortuser} {pphabdiff}{smallsummary}\n",
            "changeset_verbose": "{cset}{branches}{onelinebookmarks}{onelinetags}{parents}{user}{ldate}{fullsummary}\n\n",
            "current": "{ifcontains(rev, revset('.'), '(@)', '')}",
            "shorthash": "{label('log.changeset', '{node|short}')}",
            "pphabdiff": "{if(phabdiff, label('log.changeset', '{pad(phabdiff, 8)} '))}",
            "shortuser": "{pad(emailuser(author), 12)}",
            "cset": "{label('log.changeset changeset.{phase}', 'changeset:   {node}')} {pphabdiff} {current}\n",
            "onelinebookmarks": "{if(bookmarks, label('log.bookmark', 'bookmarks:   {join(bookmarks, ', ')}\n'), '')}",
            "joinedtags": "{join(tags, ', ')}",
            "interestingtags": "{if(joinedtags, ifeq(joinedtags, 'tip', '', joinedtags), '')}",
            "onelinetags": "{if(interestingtags, label('log.tag', 'tags:        {interestingtagstags}\n'), '')}",
            "parent": "{label('log.parent changeset.{phase}', 'parent:      {node}')}\n",
            "branch": "{label('log.branch', 'branch:      {branch}')}\n",
            "user": "{label('log.user', 'user:        {author}')}\n",
            "summary": "{if(desc|strip, '{label('log.summary', 'summary:     {desc|firstline}')}\n')}",
            "fullsummary": "{if(desc|strip, '{label('log.summary', '\n{indent(desc, '    ', '    ')}')}')}",
            "smallsummary": "{firstline(fill68(firstline(desc)))}",
            "ldate": "{label('log.date', 'date:        {date|localdate|rfc822date}')}\n",
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
            "changeset": "{cset}{bookmarks}{lphase}{user}{ldate}{summary}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("commit:      {node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "fullcset": '{labelcset("commit:      {node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{bookmarks}{lphase}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{bookmarks}{lphase}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
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
            "changeset": "{cset}{bookmarks}{user}{ldate}{summary}\\n",
            "cset_namespace": "{names_others}",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": '{if(files,\nlabel("ui.note log.files",\n"files:       {files}\\n"))}',
            "lfile_adds": '{if(file_adds,\nlabel("ui.debug log.files",\n"files+:      {file_adds}\\n"))}',
            "names_tags": "{if(names % \"{ifeq(name, 'tip', '', name)}\", \" ({label('log.{colorname}', join(names % \"{ifeq(name, 'tip', '', name)}\", ' '))})\")}",
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "showbookmarks": '{if(active, "*", " ")} {pad(bookmark, longestbookmarklen + 4)}{shortest(node, nodelen)}\\n',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "cset": '{labelcset("commit:      {node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "fullcset": '{labelcset("commit:      {node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "showwork": "{cset_shortnode}{namespaces % cset_namespace} {cset_shortdesc}",
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {node|formatnode}")}\\n',
            "cset_shortnode": "{labelcset(shortest(node, nodelen))}",
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": '{if(file_dels,\nlabel("ui.debug log.files",\n"files-:      {file_dels}\\n"))}',
            "lnode": '{label("log.node",\n"{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "names_others": "{if(names, \" ({label('log.{colorname}', join(names, ' '))})\")}",
            "changeset_verbose": "{cset}{bookmarks}{user}{ldate}{lfiles}{lfile_copies_switch}{description}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "showstack": "{showwork}",
            "changeset_debug": "{fullcset}{bookmarks}{lphase}{manifest}{user}{ldate}{lfile_mods}{lfile_adds}{lfile_dels}{lfile_copies_switch}{extras}{description}\\n",
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
            "changeset": "{cset}{bookmarks}{user}{ldate}{summary}{lfiles}\\n",
            "ldate": '{label("log.date",\n"date:        {date|date}")}\\n',
            "lfiles": "{if(files,\nlabel('ui.note log.files',\n'files:\\n'))}{lfile_mods}{lfile_adds}{lfile_dels}",
            "lfile_adds": '{file_adds % "{lfile_add}{lfile_src}"}',
            "manifest": '{label("ui.debug log.manifest",\n"manifest:    {node}")}\\n',
            "lfile_mod": '{label("status.modified", "M {file}\\n")}',
            "lfile_src": '{ifcontains(file, file_copies_switch,\nlabel("status.copied", "  {get(file_copies_switch, file)}\\n"))}',
            "bookmark": '{label("log.bookmark",\n"bookmark:    {bookmark}")}\\n',
            "extra": '{label("ui.debug log.extra",\n"extra:       {key}={value|stringescape}")}\\n',
            "lfile_add": '{label("status.added", "A {file}\\n")}',
            "cset": '{labelcset("commit:      {node|short}")}\\n',
            "changeset_quiet": "{lnode}",
            "fullcset": '{labelcset("commit:      {node}")}\\n',
            "status": '{status} {path}\\n{if(copy, "  {copy}\\n")}',
            "lfile_copies_switch": '{if(file_copies_switch,\nlabel("ui.note log.copies",\n"copies:     {file_copies_switch\n% \' {name} ({source})\'}\\n"))}',
            "description": "{if(desc|strip, \"{label('ui.note log.description',\n'description:')}\n{label('ui.note log.description',\n'{desc|strip}')}\\n\\n\")}",
            "parent": '{label("log.parent changeset.{phase}",\n"parent:      {node|formatnode}")}\\n',
            "user": '{label("log.user",\n"user:        {author}")}\\n',
            "lfile_dels": "{file_dels % \"{label('status.removed', 'R {file}\\n')}\"}",
            "lnode": '{label("log.node",\n"{node|short}")}\\n',
            "summary": "{if(desc|strip, \"{label('log.summary',\n'summary:     {desc|firstline}')}\\n\")}",
            "changeset_verbose": "{cset}{bookmarks}{user}{ldate}{description}{lfiles}\\n",
            "lphase": '{label("log.phase",\n"phase:       {phase}")}\\n',
            "changeset_debug": "{fullcset}{bookmarks}{lphase}{manifest}{user}{ldate}{extras}{description}{lfiles}\\n",
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
            "changeset": '<logentry node="{node}">\\n{bookmarks}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n</logentry>\\n',
            "parent": '<parent node="{node}" />\\n',
            "extra": '<extra key="{key|xmlescape}">{value|xmlescape}</extra>\\n',
            "bookmark": "<bookmark>{bookmark|xmlescape}</bookmark>\\n",
            "start_file_copies": "<copies>\\n",
            "file_mod": '<path action="M">{file_mod|xmlescape}</path>\\n',
            "file_del": '<path action="R">{file_del|xmlescape}</path>\\n',
            "changeset_verbose": '<logentry node="{node}">\\n{bookmarks}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n<paths>\\n{file_adds}{file_dels}{file_mods}</paths>\\n{file_copies}</logentry>\\n',
            "file_add": '<path action="A">{file_add|xmlescape}</path>\\n',
            "changeset_debug": '<logentry node="{node}">\\n{bookmarks}<author email="{author|email|xmlescape}">{author|person|xmlescape}</author>\\n<date>{date|rfc3339date}</date>\\n<msg xml:space="preserve">{desc|xmlescape}</msg>\\n<paths>\\n{file_adds}{file_dels}{file_mods}</paths>\\n{file_copies}{extras}</logentry>\\n',
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
    "commit:      %s\n"
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
    "parent": "{node|formatnode} ",
    "manifest": "{node|formatnode}",
    "file_copy": "{name} ({source})",
    "envvar": "{key}={value}",
    "extra": "{key}={value|stringescape}",
    "nodechange": "{oldnode|short} -> "
    '{join(newnodes % "{newnode|short}", ", ")|nonempty}\n',
}

# filecopy is preserved for compatibility reasons
defaulttempl["filecopy"] = defaulttempl["file_copy"]
