# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import datetime

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require("false")  # test not passing
sh % "setconfig 'extensions.treemanifest=!'"
sh % ". helpers-usechg.sh"

sh % "setconfig 'ui.allowemptycommit=1'"

sh % "hg init a"
sh % "cd a"
sh % "echo a" > "a"
sh % "hg add a"
sh % "echo line 1" > "b"
sh % "echo line 2" >> "b"
sh % "hg commit -l b -d '1000000 0' -u 'User Name <user@hostname>'"

sh % "hg add b"
sh % "echo other 1" > "c"
sh % "echo other 2" >> "c"
sh % "echo" >> "c"
sh % "echo other 3" >> "c"
sh % "hg commit -l c -d '1100000 0' -u 'A. N. Other <other@place>'"

sh % "hg add c"
sh % "hg commit -m 'no person' -d '1200000 0' -u 'other@place'"
sh % "echo c" >> "c"
sh % "hg commit -m 'no user, no domain' -d '1300000 0' -u person"

sh % "hg commit -m 'new branch' -d '1400000 0' -u person"
sh % "hg bookmark foo"

sh % "hg co -q 3"
sh % "echo other 4" >> "d"
sh % "hg add d"
sh % "hg commit -m 'new head' -d '1500000 0' -u person"

sh % "hg merge -q foo"
sh % "hg commit -m merge -d '1500001 0' -u person"

sh % "hg log -r . -T '{username}'" == "test (no-eol)"

# Test arithmetic operators have the right precedence:

sh % 'hg log -l 1 -T \'{date(date, "%Y") + 5 * 10} {date(date, "%Y") - 2 * 3}\\n\'' == "2020 1964"
sh % 'hg log -l 1 -T \'{date(date, "%Y") * 5 + 10} {date(date, "%Y") * 3 - 2}\\n\'' == "9860 5908"

# Test division:

sh % "hg debugtemplate -r0 -v '{5 / 2} {mod(5, 2)}\\n'" == r"""
    (template
      (/
        (integer '5')
        (integer '2'))
      (string ' ')
      (func
        (symbol 'mod')
        (list
          (integer '5')
          (integer '2')))
      (string '\n'))
    2 1"""
sh % "hg debugtemplate -r0 -v '{5 / -2} {mod(5, -2)}\\n'" == r"""
    (template
      (/
        (integer '5')
        (negate
          (integer '2')))
      (string ' ')
      (func
        (symbol 'mod')
        (list
          (integer '5')
          (negate
            (integer '2'))))
      (string '\n'))
    -3 -1"""
sh % "hg debugtemplate -r0 -v '{-5 / 2} {mod(-5, 2)}\\n'" == r"""
    (template
      (/
        (negate
          (integer '5'))
        (integer '2'))
      (string ' ')
      (func
        (symbol 'mod')
        (list
          (negate
            (integer '5'))
          (integer '2')))
      (string '\n'))
    -3 1"""
sh % "hg debugtemplate -r0 -v '{-5 / -2} {mod(-5, -2)}\\n'" == r"""
    (template
      (/
        (negate
          (integer '5'))
        (negate
          (integer '2')))
      (string ' ')
      (func
        (symbol 'mod')
        (list
          (negate
            (integer '5'))
          (negate
            (integer '2'))))
      (string '\n'))
    2 -1"""

# Filters bind closer than arithmetic:

sh % "hg debugtemplate -r0 -v '{revset(\".\")|count - 1}\\n'" == r"""
    (template
      (-
        (|
          (func
            (symbol 'revset')
            (string '.'))
          (symbol 'count'))
        (integer '1'))
      (string '\n'))
    0"""

# But negate binds closer still:

sh % "hg debugtemplate -r0 -v '{1-3|stringify}\\n'" == r"""
    (template
      (-
        (integer '1')
        (|
          (integer '3')
          (symbol 'stringify')))
      (string '\n'))
    hg: parse error: arithmetic only defined on integers
    [255]"""
sh % "hg debugtemplate -r0 -v '{-3|stringify}\\n'" == r"""
    (template
      (|
        (negate
          (integer '3'))
        (symbol 'stringify'))
      (string '\n'))
    -3"""

# Filters bind as close as map operator:

sh % "hg debugtemplate -r0 -v '{desc|splitlines % \"{line}\\n\"}'" == r"""
    (template
      (%
        (|
          (symbol 'desc')
          (symbol 'splitlines'))
        (template
          (symbol 'line')
          (string '\n'))))
    line 1
    line 2"""

# Keyword arguments:

sh % "hg debugtemplate -r0 -v '{foo=bar|baz}'" == r"""
    (template
      (keyvalue
        (symbol 'foo')
        (|
          (symbol 'bar')
          (symbol 'baz'))))
    hg: parse error: can't use a key-value pair in this context
    [255]"""

sh % "hg debugtemplate '{pad(\"foo\", width=10, left=true)}\\n'" == "       foo"

# Call function which takes named arguments by filter syntax:

sh % "hg debugtemplate '{\" \"|separate}'"
sh % 'hg debugtemplate \'{("not", "an", "argument", "list")|separate}\'' == r"""
    hg: parse error: unknown method 'list'
    [255]"""

# Second branch starting at nullrev:

sh % "hg update null" == "0 files updated, 0 files merged, 4 files removed, 0 files unresolved"
sh % "echo second" > "second"
sh % "hg add second"
sh % "hg commit -m second -d '1000000 0' -u 'User Name <user@hostname>'"

sh % "echo third" > "third"
sh % "hg add third"
sh % "hg mv second fourth"
sh % "hg commit -m third -d '2020-01-01 10:01'"

sh % "hg log --template '{join(file_copies, \",\\n\")}\\n' -r ." == "fourth (second)"
sh % "hg log -T '{file_copies % \"{source} -> {name}\\n\"}' -r ." == "second -> fourth"
sh % 'hg log -T \'{rev} {ifcontains("fourth", file_copies, "t", "f")}\\n\' -r \'.:7\'' == r"""
    8 t
    7 f"""

# Working-directory revision has special identifiers, though they are still
# experimental:

sh % "hg log -r 'wdir()' -T '{rev}:{node}\\n'" == "2147483647:ffffffffffffffffffffffffffffffffffffffff"

# Some keywords are invalid for working-directory revision, but they should
# never cause crash:

sh % "hg log -r 'wdir()' -T '{manifest}\\n'"

# Quoting for ui.logtemplate

sh % "hg tip --config 'ui.logtemplate={rev}\\n'" == "8"
sh % "hg tip --config 'ui.logtemplate='\\''{rev}\\n'\\'''" == "8"
sh % "hg tip --config 'ui.logtemplate=\"{rev}\\n\"'" == "8"
sh % "hg tip --config 'ui.logtemplate=n{rev}\\n'" == "n8"

# Make sure user/global hgrc does not affect tests

sh % "echo '[ui]'" > ".hg/hgrc"
sh % "echo 'logtemplate ='" >> ".hg/hgrc"
sh % "echo 'style ='" >> ".hg/hgrc"

# Add some simple styles to settings

sh % "cat" << r"""
[templates]
simple = "{rev}\n"
simple2 = {rev}\n
rev = "should not precede {rev} keyword\n"
""" >> ".hg/hgrc"

sh % "hg log -l1 -Tsimple" == "8"
sh % "hg log -l1 -Tsimple2" == "8"
sh % "hg log -l1 -Trev" == "should not precede 8 keyword"
sh % "hg log -l1 -T '{simple}'" == "8"

# Map file shouldn't see user templates:

sh % "cat" << r"""
changeset = 'nothing expanded:{simple}\n'
""" > "tmpl"
sh % "hg log -l1 --style ./tmpl" == "nothing expanded:"

# Test templates and style maps in files:

sh % "echo '{rev}'" > "tmpl"
sh % "hg log -l1 -T./tmpl" == "8"
sh % "hg log -l1 -Tblah/blah" == "blah/blah (no-eol)"

sh % "printf 'changeset = \"{rev}\\\\n\"\\n'" > "map-simple"
sh % "hg log -l1 -T./map-simple" == "8"

#  a map file may have [templates] and [templatealias] sections:

sh % "cat" << r"""
[templates]
changeset = "{a}\n"
[templatealias]
a = rev
""" > "map-simple"
sh % "hg log -l1 -T./map-simple" == "8"

#  so it can be included in hgrc

sh % "cat" << r"""
%include map-simple
[templates]
foo = "{changeset}"
""" > "myhgrc"
sh % "'HGRCPATH=./myhgrc' hg log -l1 -Tfoo" == "8"
sh % "'HGRCPATH=./myhgrc' hg log -l1 '-T{a}\\n'" == "8"

# Test template map inheritance

sh % "echo '__base__ = map-cmdline.default'" > "map-simple"
sh % "printf 'cset = \"changeset: ***{rev}***\\\\n\"\\n'" >> "map-simple"
sh % "hg log -l1 -T./map-simple" == r"""
    changeset: ***8***
    tag:         tip
    user:        test
    date:        Wed Jan 01 10:01:00 2020 +0000
    summary:     third"""

# Test docheader, docfooter and separator in template map

sh % "cat" << r"""
docheader = '\{\n'
docfooter = '\n}\n'
separator = ',\n'
changeset = ' {dict(rev, node|short)|json}'
""" > "map-myjson"
sh % "hg log -l2 -T./map-myjson" == r"""
    {
     {"node": "95c24699272e", "rev": 8},
     {"node": "29114dbae42b", "rev": 7}
    }"""

# Test docheader, docfooter and separator in [templates] section

sh % "cat" << r"""
[templates]
myjson = ' {dict(rev, node|short)|json}'
myjson:docheader = '\{\n'
myjson:docfooter = '\n}\n'
myjson:separator = ',\n'
:docheader = 'should not be selected as a docheader for literal templates\n'
""" >> ".hg/hgrc"
sh % "hg log -l2 -Tmyjson" == r"""
    {
     {"node": "95c24699272e", "rev": 8},
     {"node": "29114dbae42b", "rev": 7}
    }"""
sh % "hg log -l1 '-T{rev}\\n'" == "8"

# Template should precede style option

sh % "hg log -l1 --style default -T '{rev}\\n'" == "8"

# Add a commit with empty description, to ensure that the templates
# below will omit the description line.

sh % "echo c" >> "c"
sh % "hg add c"
sh % "hg commit -qm ' '"

# Default style is like normal output. Phases style should be the same
# as default style, except for extra phase lines.

sh % "hg log" > "log.out"
sh % "hg log --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg log -T phases" > "phases.out"
sh % "diff -U 0 log.out phases.out '|' egrep -v '^---|^\\+\\+\\+|^@@'" == r"""
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft"""

sh % "hg log -v" > "log.out"
sh % "hg log -v --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg log -v -T phases" > "phases.out"
sh % "diff -U 0 log.out phases.out '|' egrep -v '^---|^\\+\\+\\+|^@@'" == r"""
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft
    +phase:       draft"""

sh % "hg log -q" > "log.out"
sh % "hg log -q --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg log -q -T phases" > "phases.out"
sh % "cmp log.out phases.out '||' diff -u log.out phases.out"

sh % "hg log --debug" > "log.out"
sh % "hg log --debug --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg log --debug -T phases" > "phases.out"
sh % "cmp log.out phases.out '||' diff -u log.out phases.out"

# Default style of working-directory revision should also be the same (but
# date may change while running tests):

sh % "hg log -r 'wdir()' '|' sed 's|^date:.*|date:|'" > "log.out"
sh % "hg log -r 'wdir()' --style default '|' sed 's|^date:.*|date:|'" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"

sh % "hg log -r 'wdir()' -v '|' sed 's|^date:.*|date:|'" > "log.out"
sh % "hg log -r 'wdir()' -v --style default '|' sed 's|^date:.*|date:|'" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"

sh % "hg log -r 'wdir()' -q" > "log.out"
sh % "hg log -r 'wdir()' -q --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"

sh % "hg log -r 'wdir()' --debug '|' sed 's|^date:.*|date:|'" > "log.out"
sh % "hg log -r 'wdir()' --debug --style default '|' sed 's|^date:.*|date:|'" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"

# Default style should also preserve color information (issue2866):

sh % "cp '$HGRCPATH' '$HGRCPATH-bak'"
sh % "cat" << r"""
[extensions]
color=
""" >> "$HGRCPATH"

sh % "hg '--color=debug' log" > "log.out"
sh % "hg '--color=debug' log --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg '--color=debug' log -T phases" > "phases.out"
sh % "diff -U 0 log.out phases.out '|' egrep -v '^---|^\\+\\+\\+|^@@'" == r"""
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]"""

sh % "hg '--color=debug' -v log" > "log.out"
sh % "hg '--color=debug' -v log --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg '--color=debug' -v log -T phases" > "phases.out"
sh % "diff -U 0 log.out phases.out '|' egrep -v '^---|^\\+\\+\\+|^@@'" == r"""
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]
    +[log.phase|phase:       draft]"""

sh % "hg '--color=debug' -q log" > "log.out"
sh % "hg '--color=debug' -q log --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg '--color=debug' -q log -T phases" > "phases.out"
sh % "cmp log.out phases.out '||' diff -u log.out phases.out"

sh % "hg '--color=debug' --debug log" > "log.out"
sh % "hg '--color=debug' --debug log --style default" > "style.out"
sh % "cmp log.out style.out '||' diff -u log.out style.out"
sh % "hg '--color=debug' --debug log -T phases" > "phases.out"
sh % "cmp log.out phases.out '||' diff -u log.out phases.out"

sh % "mv '$HGRCPATH-bak' '$HGRCPATH'"
# Remove commit with empty commit message, so as to not pollute further
# tests.

sh % "hg debugstrip -q ."

# Revision with no copies (used to print a traceback):

sh % "hg tip -v --template '\\n'"

# Compact style works:

sh % "hg log -Tcompact" == r"""
    8[tip]   95c24699272e   2020-01-01 10:01 +0000   test
      third

    7:-1   29114dbae42b   1970-01-12 13:46 +0000   user
      second

    6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
      merge

    5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
      new head

    4[foo]   07fa1db10648   1970-01-17 04:53 +0000   person
      new branch

    3   10e46f2dcbf4   1970-01-16 01:06 +0000   person
      no user, no domain

    2   97054abb4ab8   1970-01-14 21:20 +0000   other
      no person

    1   b608e9d1a3f0   1970-01-13 17:33 +0000   other
      other 1

    0   1e4e1b8f71e0   1970-01-12 13:46 +0000   user
      line 1"""

sh % "hg log -v --style compact" == r"""
    8[tip]   95c24699272e   2020-01-01 10:01 +0000   test
      third

    7:-1   29114dbae42b   1970-01-12 13:46 +0000   User Name <user@hostname>
      second

    6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
      merge

    5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
      new head

    4   07fa1db10648   1970-01-17 04:53 +0000   person
      new branch

    3   10e46f2dcbf4   1970-01-16 01:06 +0000   person
      no user, no domain

    2   97054abb4ab8   1970-01-14 21:20 +0000   other@place
      no person

    1   b608e9d1a3f0   1970-01-13 17:33 +0000   A. N. Other <other@place>
      other 1
    other 2

    other 3

    0   1e4e1b8f71e0   1970-01-12 13:46 +0000   User Name <user@hostname>
      line 1
    line 2"""

sh % "hg log --debug --style compact" == r"""
    8[tip]:7,-1   95c24699272e   2020-01-01 10:01 +0000   test
      third

    7:-1,-1   29114dbae42b   1970-01-12 13:46 +0000   User Name <user@hostname>
      second

    6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
      merge

    5:3,-1   13207e5a10d9   1970-01-18 08:40 +0000   person
      new head

    4:3,-1   07fa1db10648   1970-01-17 04:53 +0000   person
      new branch

    3:2,-1   10e46f2dcbf4   1970-01-16 01:06 +0000   person
      no user, no domain

    2:1,-1   97054abb4ab8   1970-01-14 21:20 +0000   other@place
      no person

    1:0,-1   b608e9d1a3f0   1970-01-13 17:33 +0000   A. N. Other <other@place>
      other 1
    other 2

    other 3

    0:-1,-1   1e4e1b8f71e0   1970-01-12 13:46 +0000   User Name <user@hostname>
      line 1
    line 2"""

# Test xml styles:

sh % "hg log --style xml -r 'not all()'" == r"""
    <?xml version="1.0"?>
    <log>
    </log>"""

sh % "hg log --style xml" == r"""
    <?xml version="1.0"?>
    <log>
    <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
    <tag>tip</tag>
    <author email="test">test</author>
    <date>2020-01-01T10:01:00+00:00</date>
    <msg xml:space="preserve">third</msg>
    </logentry>
    <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">second</msg>
    </logentry>
    <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
    <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
    <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:01+00:00</date>
    <msg xml:space="preserve">merge</msg>
    </logentry>
    <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
    <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:00+00:00</date>
    <msg xml:space="preserve">new head</msg>
    </logentry>
    <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
    <bookmark>foo</bookmark>
    <author email="person">person</author>
    <date>1970-01-17T04:53:20+00:00</date>
    <msg xml:space="preserve">new branch</msg>
    </logentry>
    <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
    <author email="person">person</author>
    <date>1970-01-16T01:06:40+00:00</date>
    <msg xml:space="preserve">no user, no domain</msg>
    </logentry>
    <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
    <author email="other@place">other</author>
    <date>1970-01-14T21:20:00+00:00</date>
    <msg xml:space="preserve">no person</msg>
    </logentry>
    <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
    <author email="other@place">A. N. Other</author>
    <date>1970-01-13T17:33:20+00:00</date>
    <msg xml:space="preserve">other 1
    other 2

    other 3</msg>
    </logentry>
    <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">line 1
    line 2</msg>
    </logentry>
    </log>"""

sh % "hg log -v --style xml" == r"""
    <?xml version="1.0"?>
    <log>
    <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
    <tag>tip</tag>
    <author email="test">test</author>
    <date>2020-01-01T10:01:00+00:00</date>
    <msg xml:space="preserve">third</msg>
    <paths>
    <path action="A">fourth</path>
    <path action="A">third</path>
    <path action="R">second</path>
    </paths>
    <copies>
    <copy source="second">fourth</copy>
    </copies>
    </logentry>
    <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">second</msg>
    <paths>
    <path action="A">second</path>
    </paths>
    </logentry>
    <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
    <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
    <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:01+00:00</date>
    <msg xml:space="preserve">merge</msg>
    <paths>
    </paths>
    </logentry>
    <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
    <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:00+00:00</date>
    <msg xml:space="preserve">new head</msg>
    <paths>
    <path action="A">d</path>
    </paths>
    </logentry>
    <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
    <bookmark>foo</bookmark>
    <author email="person">person</author>
    <date>1970-01-17T04:53:20+00:00</date>
    <msg xml:space="preserve">new branch</msg>
    <paths>
    </paths>
    </logentry>
    <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
    <author email="person">person</author>
    <date>1970-01-16T01:06:40+00:00</date>
    <msg xml:space="preserve">no user, no domain</msg>
    <paths>
    <path action="M">c</path>
    </paths>
    </logentry>
    <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
    <author email="other@place">other</author>
    <date>1970-01-14T21:20:00+00:00</date>
    <msg xml:space="preserve">no person</msg>
    <paths>
    <path action="A">c</path>
    </paths>
    </logentry>
    <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
    <author email="other@place">A. N. Other</author>
    <date>1970-01-13T17:33:20+00:00</date>
    <msg xml:space="preserve">other 1
    other 2

    other 3</msg>
    <paths>
    <path action="A">b</path>
    </paths>
    </logentry>
    <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">line 1
    line 2</msg>
    <paths>
    <path action="A">a</path>
    </paths>
    </logentry>
    </log>"""

sh % "hg log --debug --style xml" == r"""
    <?xml version="1.0"?>
    <log>
    <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
    <tag>tip</tag>
    <parent revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="test">test</author>
    <date>2020-01-01T10:01:00+00:00</date>
    <msg xml:space="preserve">third</msg>
    <paths>
    <path action="A">fourth</path>
    <path action="A">third</path>
    <path action="R">second</path>
    </paths>
    <copies>
    <copy source="second">fourth</copy>
    </copies>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">second</msg>
    <paths>
    <path action="A">second</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
    <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
    <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:01+00:00</date>
    <msg xml:space="preserve">merge</msg>
    <paths>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
    <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="person">person</author>
    <date>1970-01-18T08:40:00+00:00</date>
    <msg xml:space="preserve">new head</msg>
    <paths>
    <path action="A">d</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
    <bookmark>foo</bookmark>
    <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="person">person</author>
    <date>1970-01-17T04:53:20+00:00</date>
    <msg xml:space="preserve">new branch</msg>
    <paths>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
    <parent revision="2" node="97054abb4ab824450e9164180baf491ae0078465" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="person">person</author>
    <date>1970-01-16T01:06:40+00:00</date>
    <msg xml:space="preserve">no user, no domain</msg>
    <paths>
    <path action="M">c</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
    <parent revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="other@place">other</author>
    <date>1970-01-14T21:20:00+00:00</date>
    <msg xml:space="preserve">no person</msg>
    <paths>
    <path action="A">c</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
    <parent revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="other@place">A. N. Other</author>
    <date>1970-01-13T17:33:20+00:00</date>
    <msg xml:space="preserve">other 1
    other 2

    other 3</msg>
    <paths>
    <path action="A">b</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <parent revision="-1" node="0000000000000000000000000000000000000000" />
    <author email="user@hostname">User Name</author>
    <date>1970-01-12T13:46:40+00:00</date>
    <msg xml:space="preserve">line 1
    line 2</msg>
    <paths>
    <path action="A">a</path>
    </paths>
    <extra key="branch">default</extra>
    </logentry>
    </log>"""


# Test JSON style:

sh % "hg log -k nosuch -Tjson" == "[]"

sh % "hg log -qr . -Tjson" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a"
     }
    ]"""

sh % "hg log -vpr . -Tjson --stat" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [1577872860, 0],
      "desc": "third",
      "bookmarks": [],
      "tags": ["tip"],
      "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
      "files": ["fourth", "second", "third"],
      "diffstat": " fourth |  1 +\n second |  1 -\n third  |  1 +\n 3 files changed, 2 insertions(+), 1 deletions(-)\n",
      "diff": "diff -r 29114dbae42b -r 95c24699272e fourth\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/fourth\tWed Jan 01 10:01:00 2020 +0000\n@@ -0,0 +1,1 @@\n+second\ndiff -r 29114dbae42b -r 95c24699272e second\n--- a/second\tMon Jan 12 13:46:40 1970 +0000\n+++ /dev/null\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,1 +0,0 @@\n-second\ndiff -r 29114dbae42b -r 95c24699272e third\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/third\tWed Jan 01 10:01:00 2020 +0000\n@@ -0,0 +1,1 @@\n+third\n"
     }
    ]"""

# honor --git but not format-breaking diffopts
sh % "hg --config 'diff.noprefix=True' log --git -vpr . -Tjson" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [1577872860, 0],
      "desc": "third",
      "bookmarks": [],
      "tags": ["tip"],
      "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
      "files": ["fourth", "second", "third"],
      "diff": "diff --git a/second b/fourth\nrename from second\nrename to fourth\ndiff --git a/third b/third\nnew file mode 100644\n--- /dev/null\n+++ b/third\n@@ -0,0 +1,1 @@\n+third\n"
     }
    ]"""

sh % "hg log -T json" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [1577872860, 0],
      "desc": "third",
      "bookmarks": [],
      "tags": ["tip"],
      "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"]
     },
     {
      "rev": 7,
      "node": "29114dbae42b9f078cf2714dbe3a86bba8ec7453",
      "branch": "default",
      "phase": "draft",
      "user": "User Name <user@hostname>",
      "date": [1000000, 0],
      "desc": "second",
      "bookmarks": [],
      "tags": [],
      "parents": ["0000000000000000000000000000000000000000"]
     },
     {
      "rev": 6,
      "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1500001, 0],
      "desc": "merge",
      "bookmarks": [],
      "tags": [],
      "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"]
     },
     {
      "rev": 5,
      "node": "13207e5a10d9fd28ec424934298e176197f2c67f",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1500000, 0],
      "desc": "new head",
      "bookmarks": [],
      "tags": [],
      "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"]
     },
     {
      "rev": 4,
      "node": "07fa1db1064879a32157227401eb44b322ae53ce",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1400000, 0],
      "desc": "new branch",
      "bookmarks": ["foo"],
      "tags": [],
      "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"]
     },
     {
      "rev": 3,
      "node": "10e46f2dcbf4823578cf180f33ecf0b957964c47",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1300000, 0],
      "desc": "no user, no domain",
      "bookmarks": [],
      "tags": [],
      "parents": ["97054abb4ab824450e9164180baf491ae0078465"]
     },
     {
      "rev": 2,
      "node": "97054abb4ab824450e9164180baf491ae0078465",
      "branch": "default",
      "phase": "draft",
      "user": "other@place",
      "date": [1200000, 0],
      "desc": "no person",
      "bookmarks": [],
      "tags": [],
      "parents": ["b608e9d1a3f0273ccf70fb85fd6866b3482bf965"]
     },
     {
      "rev": 1,
      "node": "b608e9d1a3f0273ccf70fb85fd6866b3482bf965",
      "branch": "default",
      "phase": "draft",
      "user": "A. N. Other <other@place>",
      "date": [1100000, 0],
      "desc": "other 1\nother 2\n\nother 3",
      "bookmarks": [],
      "tags": [],
      "parents": ["1e4e1b8f71e05681d422154f5421e385fec3454f"]
     },
     {
      "rev": 0,
      "node": "1e4e1b8f71e05681d422154f5421e385fec3454f",
      "branch": "default",
      "phase": "draft",
      "user": "User Name <user@hostname>",
      "date": [1000000, 0],
      "desc": "line 1\nline 2",
      "bookmarks": [],
      "tags": [],
      "parents": ["0000000000000000000000000000000000000000"]
     }
    ]"""

sh % "hg heads -v -Tjson" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [1577872860, 0],
      "desc": "third",
      "bookmarks": [],
      "tags": ["tip"],
      "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
      "files": ["fourth", "second", "third"]
     },
     {
      "rev": 6,
      "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1500001, 0],
      "desc": "merge",
      "bookmarks": [],
      "tags": [],
      "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"],
      "files": []
     }
    ]"""

sh % "hg log --debug -Tjson" == r"""
    [
     {
      "rev": 8,
      "node": "95c24699272ef57d062b8bccc32c878bf841784a",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [1577872860, 0],
      "desc": "third",
      "bookmarks": [],
      "tags": ["tip"],
      "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
      "manifest": "94961b75a2da554b4df6fb599e5bfc7d48de0c64",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["fourth", "third"],
      "removed": ["second"]
     },
     {
      "rev": 7,
      "node": "29114dbae42b9f078cf2714dbe3a86bba8ec7453",
      "branch": "default",
      "phase": "draft",
      "user": "User Name <user@hostname>",
      "date": [1000000, 0],
      "desc": "second",
      "bookmarks": [],
      "tags": [],
      "parents": ["0000000000000000000000000000000000000000"],
      "manifest": "f2dbc354b94e5ec0b4f10680ee0cee816101d0bf",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["second"],
      "removed": []
     },
     {
      "rev": 6,
      "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1500001, 0],
      "desc": "merge",
      "bookmarks": [],
      "tags": [],
      "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"],
      "manifest": "4dc3def4f9b4c6e8de820f6ee74737f91e96a216",
      "extra": {"branch": "default"},
      "modified": [],
      "added": [],
      "removed": []
     },
     {
      "rev": 5,
      "node": "13207e5a10d9fd28ec424934298e176197f2c67f",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1500000, 0],
      "desc": "new head",
      "bookmarks": [],
      "tags": [],
      "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
      "manifest": "4dc3def4f9b4c6e8de820f6ee74737f91e96a216",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["d"],
      "removed": []
     },
     {
      "rev": 4,
      "node": "07fa1db1064879a32157227401eb44b322ae53ce",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1400000, 0],
      "desc": "new branch",
      "bookmarks": ["foo"],
      "tags": [],
      "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
      "manifest": "cb5a1327723bada42f117e4c55a303246eaf9ccc",
      "extra": {"branch": "default"},
      "modified": [],
      "added": [],
      "removed": []
     },
     {
      "rev": 3,
      "node": "10e46f2dcbf4823578cf180f33ecf0b957964c47",
      "branch": "default",
      "phase": "draft",
      "user": "person",
      "date": [1300000, 0],
      "desc": "no user, no domain",
      "bookmarks": [],
      "tags": [],
      "parents": ["97054abb4ab824450e9164180baf491ae0078465"],
      "manifest": "cb5a1327723bada42f117e4c55a303246eaf9ccc",
      "extra": {"branch": "default"},
      "modified": ["c"],
      "added": [],
      "removed": []
     },
     {
      "rev": 2,
      "node": "97054abb4ab824450e9164180baf491ae0078465",
      "branch": "default",
      "phase": "draft",
      "user": "other@place",
      "date": [1200000, 0],
      "desc": "no person",
      "bookmarks": [],
      "tags": [],
      "parents": ["b608e9d1a3f0273ccf70fb85fd6866b3482bf965"],
      "manifest": "6e0e82995c35d0d57a52aca8da4e56139e06b4b1",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["c"],
      "removed": []
     },
     {
      "rev": 1,
      "node": "b608e9d1a3f0273ccf70fb85fd6866b3482bf965",
      "branch": "default",
      "phase": "draft",
      "user": "A. N. Other <other@place>",
      "date": [1100000, 0],
      "desc": "other 1\nother 2\n\nother 3",
      "bookmarks": [],
      "tags": [],
      "parents": ["1e4e1b8f71e05681d422154f5421e385fec3454f"],
      "manifest": "4e8d705b1e53e3f9375e0e60dc7b525d8211fe55",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["b"],
      "removed": []
     },
     {
      "rev": 0,
      "node": "1e4e1b8f71e05681d422154f5421e385fec3454f",
      "branch": "default",
      "phase": "draft",
      "user": "User Name <user@hostname>",
      "date": [1000000, 0],
      "desc": "line 1\nline 2",
      "bookmarks": [],
      "tags": [],
      "parents": ["0000000000000000000000000000000000000000"],
      "manifest": "a0c8bcbbb45c63b90b70ad007bf38961f64f2af0",
      "extra": {"branch": "default"},
      "modified": [],
      "added": ["a"],
      "removed": []
     }
    ]"""

# Error if style not readable:

if feature.check(["unix-permissions", "no-root"]):
    sh % "touch q"
    sh % "chmod 0 q"
    sh % "hg log --style ./q" == r"""
        abort: Permission denied: ./q
        [255]"""


# Error if no style:

sh % "hg log --style notexist" == r"""
    abort: style 'notexist' not found
    (available styles: bisect, changelog, compact, default, phases, show, status, xml)
    [255]"""

sh % "hg log -T list" == r"""
    available styles: bisect, changelog, compact, default, phases, show, status, xml
    abort: specify a template
    [255]"""

# Error if style missing key:

sh % "echo 'q = q'" > "t"
sh % "hg log --style ./t" == r"""
    abort: "changeset" not in template map
    [255]"""

# Error if style missing value:

sh % "echo 'changeset ='" > "t"
sh % "hg log --style t" == r"""
    hg: parse error at t:1: missing value
    [255]"""

# Error if include fails:

sh % "echo 'changeset = q'" >> "t"
if feature.check(["unix-permissions", "no-root"]):
    sh % "hg log --style ./t" == r"""
        abort: template file ./q: Permission denied
        [255]"""
    sh % "rm -f q"


# Include works:

sh % "echo '{rev}'" > "q"
sh % "hg log --style ./t" == r"""
    8
    7
    6
    5
    4
    3
    2
    1
    0"""

# Check that recursive reference does not fall into RuntimeError (issue4758):

#  common mistake:

sh % "cat" << r"""
changeset = '{changeset}\n'
""" > "issue4758"
sh % "hg log --style ./issue4758" == r"""
    abort: recursive reference 'changeset' in template
    [255]"""

#  circular reference:

sh % "cat" << r"""
changeset = '{foo}'
foo = '{changeset}'
""" > "issue4758"
sh % "hg log --style ./issue4758" == r"""
    abort: recursive reference 'foo' in template
    [255]"""

#  buildmap() -> gettemplate(), where no thunk was made:

sh % "cat" << r"""
changeset = '{files % changeset}\n'
""" > "issue4758"
sh % "hg log --style ./issue4758" == r"""
    abort: recursive reference 'changeset' in template
    [255]"""

#  not a recursion if a keyword of the same name exists:

sh % "cat" << r"""
changeset = '{tags % rev}'
rev = '{rev} {tag}\n'
""" > "issue4758"
sh % "hg log --style ./issue4758 -r tip" == "8 tip"

# Check that {phase} works correctly on parents:

sh % "cat" << r"""
changeset_debug = '{rev} ({phase}):{parents}\n'
parent = ' {rev} ({phase})'
""" > "parentphase"
sh % "hg phase -r 5 --public"
sh % "hg phase -r 7 --secret --force"
sh % "hg log --debug -G --style ./parentphase" == r"""
    @  8 (secret): 7 (secret) -1 (public)
    |
    o  7 (secret): -1 (public) -1 (public)

    o    6 (draft): 5 (public) 4 (draft)
    |\
    | o  5 (public): 3 (public) -1 (public)
    | |
    o |  4 (draft): 3 (public) -1 (public)
    |/
    o  3 (public): 2 (public) -1 (public)
    |
    o  2 (public): 1 (public) -1 (public)
    |
    o  1 (public): 0 (public) -1 (public)
    |
    o  0 (public): -1 (public) -1 (public)"""

# Missing non-standard names give no error (backward compatibility):

sh % "echo 'changeset = '\\''{c}'\\'''" > "t"
sh % "hg log --style ./t"

# Defining non-standard name works:

sh % "cat" << r"""
changeset = '{c}'
c = q
""" > "t"
sh % "hg log --style ./t" == r"""
    8
    7
    6
    5
    4
    3
    2
    1
    0"""

# ui.style works:

sh % "echo '[ui]'" > ".hg/hgrc"
sh % "echo 'style = t'" >> ".hg/hgrc"
sh % "hg log" == r"""
    8
    7
    6
    5
    4
    3
    2
    1
    0"""


# Issue338:

sh % "hg log '--style=changelog'" > "changelog"

sh % "cat changelog" == r"""
    2020-01-01  test  <test>

    	* fourth, second, third:
    	third
    	[95c24699272e] [tip]

    1970-01-12  User Name  <user@hostname>

    	* second:
    	second
    	[29114dbae42b]

    1970-01-18  person  <person>

    	* merge
    	[f7e5795620e7]

    	* d:
    	new head
    	[13207e5a10d9]

    1970-01-17  person  <person>

    	* new branch
    	[07fa1db10648]

    1970-01-16  person  <person>

    	* c:
    	no user, no domain
    	[10e46f2dcbf4]

    1970-01-14  other  <other@place>

    	* c:
    	no person
    	[97054abb4ab8]

    1970-01-13  A. N. Other  <other@place>

    	* b:
    	other 1 other 2

    	other 3
    	[b608e9d1a3f0]

    1970-01-12  User Name  <user@hostname>

    	* a:
    	line 1 line 2
    	[1e4e1b8f71e0]"""

# Issue2130: xml output for 'hg heads' is malformed

sh % "hg heads --style changelog" == r"""
    2020-01-01  test  <test>

    	* fourth, second, third:
    	third
    	[95c24699272e] [tip]

    1970-01-18  person  <person>

    	* merge
    	[f7e5795620e7]"""

# Keys work:

sh % "for key in author branch branches date desc file_adds file_dels file_mods file_copies file_copies_switch files manifest node parents rev tags diffstat extras p1rev p2rev p1node 'p2node;' do" == r"""
    >     for mode in '' --verbose --debug; do
    >         hg log $mode --template "$key$mode: {$key}\n"
    >     done
    > done
    author: test
    author: User Name <user@hostname>
    author: person
    author: person
    author: person
    author: person
    author: other@place
    author: A. N. Other <other@place>
    author: User Name <user@hostname>
    author--verbose: test
    author--verbose: User Name <user@hostname>
    author--verbose: person
    author--verbose: person
    author--verbose: person
    author--verbose: person
    author--verbose: other@place
    author--verbose: A. N. Other <other@place>
    author--verbose: User Name <user@hostname>
    author--debug: test
    author--debug: User Name <user@hostname>
    author--debug: person
    author--debug: person
    author--debug: person
    author--debug: person
    author--debug: other@place
    author--debug: A. N. Other <other@place>
    author--debug: User Name <user@hostname>
    branch: default
    branch: default
    branch: default
    branch: default
    branch: default
    branch: default
    branch: default
    branch: default
    branch: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--verbose: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branch--debug: default
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--verbose:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    branches--debug:  (trailing space)
    date: 1577872860.00
    date: 1000000.00
    date: 1500001.00
    date: 1500000.00
    date: 1400000.00
    date: 1300000.00
    date: 1200000.00
    date: 1100000.00
    date: 1000000.00
    date--verbose: 1577872860.00
    date--verbose: 1000000.00
    date--verbose: 1500001.00
    date--verbose: 1500000.00
    date--verbose: 1400000.00
    date--verbose: 1300000.00
    date--verbose: 1200000.00
    date--verbose: 1100000.00
    date--verbose: 1000000.00
    date--debug: 1577872860.00
    date--debug: 1000000.00
    date--debug: 1500001.00
    date--debug: 1500000.00
    date--debug: 1400000.00
    date--debug: 1300000.00
    date--debug: 1200000.00
    date--debug: 1100000.00
    date--debug: 1000000.00
    desc: third
    desc: second
    desc: merge
    desc: new head
    desc: new branch
    desc: no user, no domain
    desc: no person
    desc: other 1
    other 2

    other 3
    desc: line 1
    line 2
    desc--verbose: third
    desc--verbose: second
    desc--verbose: merge
    desc--verbose: new head
    desc--verbose: new branch
    desc--verbose: no user, no domain
    desc--verbose: no person
    desc--verbose: other 1
    other 2

    other 3
    desc--verbose: line 1
    line 2
    desc--debug: third
    desc--debug: second
    desc--debug: merge
    desc--debug: new head
    desc--debug: new branch
    desc--debug: no user, no domain
    desc--debug: no person
    desc--debug: other 1
    other 2

    other 3
    desc--debug: line 1
    line 2
    file_adds: fourth third
    file_adds: second
    file_adds:  (trailing space)
    file_adds: d
    file_adds:  (trailing space)
    file_adds:  (trailing space)
    file_adds: c
    file_adds: b
    file_adds: a
    file_adds--verbose: fourth third
    file_adds--verbose: second
    file_adds--verbose:  (trailing space)
    file_adds--verbose: d
    file_adds--verbose:  (trailing space)
    file_adds--verbose:  (trailing space)
    file_adds--verbose: c
    file_adds--verbose: b
    file_adds--verbose: a
    file_adds--debug: fourth third
    file_adds--debug: second
    file_adds--debug:  (trailing space)
    file_adds--debug: d
    file_adds--debug:  (trailing space)
    file_adds--debug:  (trailing space)
    file_adds--debug: c
    file_adds--debug: b
    file_adds--debug: a
    file_dels: second
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels:  (trailing space)
    file_dels--verbose: second
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--verbose:  (trailing space)
    file_dels--debug: second
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_dels--debug:  (trailing space)
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods: c
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose: c
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--verbose:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug: c
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_mods--debug:  (trailing space)
    file_copies: fourth (second)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies:  (trailing space)
    file_copies--verbose: fourth (second)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--verbose:  (trailing space)
    file_copies--debug: fourth (second)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies--debug:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--verbose:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    file_copies_switch--debug:  (trailing space)
    files: fourth second third
    files: second
    files:  (trailing space)
    files: d
    files:  (trailing space)
    files: c
    files: c
    files: b
    files: a
    files--verbose: fourth second third
    files--verbose: second
    files--verbose:  (trailing space)
    files--verbose: d
    files--verbose:  (trailing space)
    files--verbose: c
    files--verbose: c
    files--verbose: b
    files--verbose: a
    files--debug: fourth second third
    files--debug: second
    files--debug:  (trailing space)
    files--debug: d
    files--debug:  (trailing space)
    files--debug: c
    files--debug: c
    files--debug: b
    files--debug: a
    manifest: 6:94961b75a2da
    manifest: 5:f2dbc354b94e
    manifest: 4:4dc3def4f9b4
    manifest: 4:4dc3def4f9b4
    manifest: 3:cb5a1327723b
    manifest: 3:cb5a1327723b
    manifest: 2:6e0e82995c35
    manifest: 1:4e8d705b1e53
    manifest: 0:a0c8bcbbb45c
    manifest--verbose: 6:94961b75a2da
    manifest--verbose: 5:f2dbc354b94e
    manifest--verbose: 4:4dc3def4f9b4
    manifest--verbose: 4:4dc3def4f9b4
    manifest--verbose: 3:cb5a1327723b
    manifest--verbose: 3:cb5a1327723b
    manifest--verbose: 2:6e0e82995c35
    manifest--verbose: 1:4e8d705b1e53
    manifest--verbose: 0:a0c8bcbbb45c
    manifest--debug: 6:94961b75a2da554b4df6fb599e5bfc7d48de0c64
    manifest--debug: 5:f2dbc354b94e5ec0b4f10680ee0cee816101d0bf
    manifest--debug: 4:4dc3def4f9b4c6e8de820f6ee74737f91e96a216
    manifest--debug: 4:4dc3def4f9b4c6e8de820f6ee74737f91e96a216
    manifest--debug: 3:cb5a1327723bada42f117e4c55a303246eaf9ccc
    manifest--debug: 3:cb5a1327723bada42f117e4c55a303246eaf9ccc
    manifest--debug: 2:6e0e82995c35d0d57a52aca8da4e56139e06b4b1
    manifest--debug: 1:4e8d705b1e53e3f9375e0e60dc7b525d8211fe55
    manifest--debug: 0:a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
    node: 95c24699272ef57d062b8bccc32c878bf841784a
    node: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    node: f7e5795620e78993ad76680c4306bb2da83907b3
    node: 13207e5a10d9fd28ec424934298e176197f2c67f
    node: 07fa1db1064879a32157227401eb44b322ae53ce
    node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    node: 97054abb4ab824450e9164180baf491ae0078465
    node: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    node: 1e4e1b8f71e05681d422154f5421e385fec3454f
    node--verbose: 95c24699272ef57d062b8bccc32c878bf841784a
    node--verbose: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    node--verbose: f7e5795620e78993ad76680c4306bb2da83907b3
    node--verbose: 13207e5a10d9fd28ec424934298e176197f2c67f
    node--verbose: 07fa1db1064879a32157227401eb44b322ae53ce
    node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    node--verbose: 97054abb4ab824450e9164180baf491ae0078465
    node--verbose: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    node--verbose: 1e4e1b8f71e05681d422154f5421e385fec3454f
    node--debug: 95c24699272ef57d062b8bccc32c878bf841784a
    node--debug: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    node--debug: f7e5795620e78993ad76680c4306bb2da83907b3
    node--debug: 13207e5a10d9fd28ec424934298e176197f2c67f
    node--debug: 07fa1db1064879a32157227401eb44b322ae53ce
    node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    node--debug: 97054abb4ab824450e9164180baf491ae0078465
    node--debug: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    node--debug: 1e4e1b8f71e05681d422154f5421e385fec3454f
    parents:  (trailing space)
    parents: -1:000000000000  (trailing space)
    parents: 5:13207e5a10d9 4:07fa1db10648  (trailing space)
    parents: 3:10e46f2dcbf4  (trailing space)
    parents:  (trailing space)
    parents:  (trailing space)
    parents:  (trailing space)
    parents:  (trailing space)
    parents:  (trailing space)
    parents--verbose:  (trailing space)
    parents--verbose: -1:000000000000  (trailing space)
    parents--verbose: 5:13207e5a10d9 4:07fa1db10648  (trailing space)
    parents--verbose: 3:10e46f2dcbf4  (trailing space)
    parents--verbose:  (trailing space)
    parents--verbose:  (trailing space)
    parents--verbose:  (trailing space)
    parents--verbose:  (trailing space)
    parents--verbose:  (trailing space)
    parents--debug: 7:29114dbae42b9f078cf2714dbe3a86bba8ec7453 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: -1:0000000000000000000000000000000000000000 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: 5:13207e5a10d9fd28ec424934298e176197f2c67f 4:07fa1db1064879a32157227401eb44b322ae53ce  (trailing space)
    parents--debug: 3:10e46f2dcbf4823578cf180f33ecf0b957964c47 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: 3:10e46f2dcbf4823578cf180f33ecf0b957964c47 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: 2:97054abb4ab824450e9164180baf491ae0078465 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: 1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965 -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: 0:1e4e1b8f71e05681d422154f5421e385fec3454f -1:0000000000000000000000000000000000000000  (trailing space)
    parents--debug: -1:0000000000000000000000000000000000000000 -1:0000000000000000000000000000000000000000  (trailing space)
    rev: 8
    rev: 7
    rev: 6
    rev: 5
    rev: 4
    rev: 3
    rev: 2
    rev: 1
    rev: 0
    rev--verbose: 8
    rev--verbose: 7
    rev--verbose: 6
    rev--verbose: 5
    rev--verbose: 4
    rev--verbose: 3
    rev--verbose: 2
    rev--verbose: 1
    rev--verbose: 0
    rev--debug: 8
    rev--debug: 7
    rev--debug: 6
    rev--debug: 5
    rev--debug: 4
    rev--debug: 3
    rev--debug: 2
    rev--debug: 1
    rev--debug: 0
    tags: tip
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags:  (trailing space)
    tags--verbose: tip
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--verbose:  (trailing space)
    tags--debug: tip
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    tags--debug:  (trailing space)
    diffstat: 3: +2/-1
    diffstat: 1: +1/-0
    diffstat: 0: +0/-0
    diffstat: 1: +1/-0
    diffstat: 0: +0/-0
    diffstat: 1: +1/-0
    diffstat: 1: +4/-0
    diffstat: 1: +2/-0
    diffstat: 1: +1/-0
    diffstat--verbose: 3: +2/-1
    diffstat--verbose: 1: +1/-0
    diffstat--verbose: 0: +0/-0
    diffstat--verbose: 1: +1/-0
    diffstat--verbose: 0: +0/-0
    diffstat--verbose: 1: +1/-0
    diffstat--verbose: 1: +4/-0
    diffstat--verbose: 1: +2/-0
    diffstat--verbose: 1: +1/-0
    diffstat--debug: 3: +2/-1
    diffstat--debug: 1: +1/-0
    diffstat--debug: 0: +0/-0
    diffstat--debug: 1: +1/-0
    diffstat--debug: 0: +0/-0
    diffstat--debug: 1: +1/-0
    diffstat--debug: 1: +4/-0
    diffstat--debug: 1: +2/-0
    diffstat--debug: 1: +1/-0
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--verbose: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    extras--debug: branch=default
    p1rev: 7
    p1rev: -1
    p1rev: 5
    p1rev: 3
    p1rev: 3
    p1rev: 2
    p1rev: 1
    p1rev: 0
    p1rev: -1
    p1rev--verbose: 7
    p1rev--verbose: -1
    p1rev--verbose: 5
    p1rev--verbose: 3
    p1rev--verbose: 3
    p1rev--verbose: 2
    p1rev--verbose: 1
    p1rev--verbose: 0
    p1rev--verbose: -1
    p1rev--debug: 7
    p1rev--debug: -1
    p1rev--debug: 5
    p1rev--debug: 3
    p1rev--debug: 3
    p1rev--debug: 2
    p1rev--debug: 1
    p1rev--debug: 0
    p1rev--debug: -1
    p2rev: -1
    p2rev: -1
    p2rev: 4
    p2rev: -1
    p2rev: -1
    p2rev: -1
    p2rev: -1
    p2rev: -1
    p2rev: -1
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--verbose: 4
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--verbose: -1
    p2rev--debug: -1
    p2rev--debug: -1
    p2rev--debug: 4
    p2rev--debug: -1
    p2rev--debug: -1
    p2rev--debug: -1
    p2rev--debug: -1
    p2rev--debug: -1
    p2rev--debug: -1
    p1node: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    p1node: 0000000000000000000000000000000000000000
    p1node: 13207e5a10d9fd28ec424934298e176197f2c67f
    p1node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node: 97054abb4ab824450e9164180baf491ae0078465
    p1node: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    p1node: 1e4e1b8f71e05681d422154f5421e385fec3454f
    p1node: 0000000000000000000000000000000000000000
    p1node--verbose: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    p1node--verbose: 0000000000000000000000000000000000000000
    p1node--verbose: 13207e5a10d9fd28ec424934298e176197f2c67f
    p1node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node--verbose: 97054abb4ab824450e9164180baf491ae0078465
    p1node--verbose: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    p1node--verbose: 1e4e1b8f71e05681d422154f5421e385fec3454f
    p1node--verbose: 0000000000000000000000000000000000000000
    p1node--debug: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
    p1node--debug: 0000000000000000000000000000000000000000
    p1node--debug: 13207e5a10d9fd28ec424934298e176197f2c67f
    p1node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
    p1node--debug: 97054abb4ab824450e9164180baf491ae0078465
    p1node--debug: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    p1node--debug: 1e4e1b8f71e05681d422154f5421e385fec3454f
    p1node--debug: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 07fa1db1064879a32157227401eb44b322ae53ce
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 07fa1db1064879a32157227401eb44b322ae53ce
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--verbose: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 07fa1db1064879a32157227401eb44b322ae53ce
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000
    p2node--debug: 0000000000000000000000000000000000000000"""

# Filters work:

sh % "hg log --template '{author|domain}\\n'" == r"""

    hostname




    place
    place
    hostname"""

sh % "hg log --template '{author|person}\\n'" == r"""
    test
    User Name
    person
    person
    person
    person
    other
    A. N. Other
    User Name"""

sh % "hg log --template '{author|user}\\n'" == r"""
    test
    user
    person
    person
    person
    person
    other
    other
    user"""

sh % "hg log --template '{date|date}\\n'" == r"""
    Wed Jan 01 10:01:00 2020 +0000
    Mon Jan 12 13:46:40 1970 +0000
    Sun Jan 18 08:40:01 1970 +0000
    Sun Jan 18 08:40:00 1970 +0000
    Sat Jan 17 04:53:20 1970 +0000
    Fri Jan 16 01:06:40 1970 +0000
    Wed Jan 14 21:20:00 1970 +0000
    Tue Jan 13 17:33:20 1970 +0000
    Mon Jan 12 13:46:40 1970 +0000"""

sh % "hg log --template '{date|isodate}\\n'" == r"""
    2020-01-01 10:01 +0000
    1970-01-12 13:46 +0000
    1970-01-18 08:40 +0000
    1970-01-18 08:40 +0000
    1970-01-17 04:53 +0000
    1970-01-16 01:06 +0000
    1970-01-14 21:20 +0000
    1970-01-13 17:33 +0000
    1970-01-12 13:46 +0000"""

sh % "hg log --template '{date|isodatesec}\\n'" == r"""
    2020-01-01 10:01:00 +0000
    1970-01-12 13:46:40 +0000
    1970-01-18 08:40:01 +0000
    1970-01-18 08:40:00 +0000
    1970-01-17 04:53:20 +0000
    1970-01-16 01:06:40 +0000
    1970-01-14 21:20:00 +0000
    1970-01-13 17:33:20 +0000
    1970-01-12 13:46:40 +0000"""

sh % "hg log --template '{date|rfc822date}\\n'" == r"""
    Wed, 01 Jan 2020 10:01:00 +0000
    Mon, 12 Jan 1970 13:46:40 +0000
    Sun, 18 Jan 1970 08:40:01 +0000
    Sun, 18 Jan 1970 08:40:00 +0000
    Sat, 17 Jan 1970 04:53:20 +0000
    Fri, 16 Jan 1970 01:06:40 +0000
    Wed, 14 Jan 1970 21:20:00 +0000
    Tue, 13 Jan 1970 17:33:20 +0000
    Mon, 12 Jan 1970 13:46:40 +0000"""

sh % "hg log --template '{desc|firstline}\\n'" == r"""
    third
    second
    merge
    new head
    new branch
    no user, no domain
    no person
    other 1
    line 1"""

sh % "hg log --template '{node|short}\\n'" == r"""
    95c24699272e
    29114dbae42b
    f7e5795620e7
    13207e5a10d9
    07fa1db10648
    10e46f2dcbf4
    97054abb4ab8
    b608e9d1a3f0
    1e4e1b8f71e0"""

sh % "hg log --template" << open(
    'changeset author="{author|xmlescape}"/>\\n'
).read() == r"""
    <changeset author="test"/>
    <changeset author="User Name &lt;user@hostname&gt;"/>
    <changeset author="person"/>
    <changeset author="person"/>
    <changeset author="person"/>
    <changeset author="person"/>
    <changeset author="other@place"/>
    <changeset author="A. N. Other &lt;other@place&gt;"/>
    <changeset author="User Name &lt;user@hostname&gt;"/>"""

sh % "hg log --template '{rev}: {children}\\n'" == r"""
    8:  (trailing space)
    7: 8:95c24699272e
    6:  (trailing space)
    5: 6:f7e5795620e7
    4: 6:f7e5795620e7
    3: 4:07fa1db10648 5:13207e5a10d9
    2: 3:10e46f2dcbf4
    1: 2:97054abb4ab8
    0: 1:b608e9d1a3f0"""

# Formatnode filter works:

sh % "hg -q log -r 0 --template '{node|formatnode}\\n'" == "1e4e1b8f71e0"

sh % "hg log -r 0 --template '{node|formatnode}\\n'" == "1e4e1b8f71e0"

sh % "hg -v log -r 0 --template '{node|formatnode}\\n'" == "1e4e1b8f71e0"

sh % "hg --debug log -r 0 --template '{node|formatnode}\\n'" == "1e4e1b8f71e05681d422154f5421e385fec3454f"

# Age filter:

sh % "hg init unstable-hash"
sh % "cd unstable-hash"
sh % "hg log --template '{date|age}\\n' '||' exit 1" > "/dev/null"


fp = open("a", "w")
n = datetime.datetime.now() + datetime.timedelta(366 * 7)
fp.write("%d-%d-%d 00:00" % (n.year, n.month, n.day))
fp.close()
sh % "hg add a"
sh % "hg commit -m future -d '`cat a`'"

sh % "hg log -l1 --template '{date|age}\\n'" == "7 years from now"

sh % "cd .."
sh % "rm -rf unstable-hash"

# Add a dummy commit to make up for the instability of the above:

sh % "echo a" > "a"
sh % "hg add a"
sh % "hg ci -m future"

# Count filter:

sh % "hg log -l1 --template '{node|count} {node|short|count}\\n'" == "40 12"

sh % 'hg log -l1 --template \'{revset("null^")|count} {revset(".")|count} {revset("0::3")|count}\\n\'' == "0 1 4"

sh % "hg log -G --template '{rev}: children: {children|count}, tags: {tags|count}, file_adds: {file_adds|count}, ancestors: {revset(\"ancestors(%s)\", rev)|count}'" == r"""
    @  9: children: 0, tags: 1, file_adds: 1, ancestors: 3
    |
    o  8: children: 1, tags: 0, file_adds: 2, ancestors: 2
    |
    o  7: children: 1, tags: 0, file_adds: 1, ancestors: 1

    o    6: children: 0, tags: 0, file_adds: 0, ancestors: 7
    |\
    | o  5: children: 1, tags: 0, file_adds: 1, ancestors: 5
    | |
    o |  4: children: 1, tags: 0, file_adds: 0, ancestors: 5
    |/
    o  3: children: 2, tags: 0, file_adds: 0, ancestors: 4
    |
    o  2: children: 1, tags: 0, file_adds: 1, ancestors: 3
    |
    o  1: children: 1, tags: 0, file_adds: 1, ancestors: 2
    |
    o  0: children: 1, tags: 0, file_adds: 1, ancestors: 1"""

# Upper/lower filters:

sh % "hg log -r0 --template '{author|upper}\\n'" == "USER NAME <USER@HOSTNAME>"
sh % "hg log -r0 --template '{author|lower}\\n'" == "user name <user@hostname>"
sh % "hg log -r0 --template '{date|upper}\\n'" == r"""
    abort: template filter 'upper' is not compatible with keyword 'date'
    [255]"""

# Add a commit that does all possible modifications at once

sh % "echo modify" >> "third"
sh % "touch b"
sh % "hg add b"
sh % "hg mv fourth fifth"
sh % "hg rm a"
sh % "hg ci -m 'Modify, add, remove, rename'"

# Check the status template

sh % "cat" << r"""
[extensions]
color=
""" >> "$HGRCPATH"

sh % "hg log -T status -r 10" == r"""
    changeset:   10:0f9759ec227a
    tag:         tip
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     Modify, add, remove, rename
    files:
    M third
    A b
    A fifth
    R a
    R fourth"""
sh % "hg log -T status -C -r 10" == r"""
    changeset:   10:0f9759ec227a
    tag:         tip
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     Modify, add, remove, rename
    files:
    M third
    A b
    A fifth
      fourth
    R a
    R fourth"""
sh % "hg log -T status -C -r 10 -v" == r"""
    changeset:   10:0f9759ec227a
    tag:         tip
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    description:
    Modify, add, remove, rename

    files:
    M third
    A b
    A fifth
      fourth
    R a
    R fourth"""
sh % "hg log -T status -C -r 10 --debug" == r"""
    changeset:   10:0f9759ec227a4859c2014a345cd8a859022b7c6c
    tag:         tip
    phase:       secret
    parent:      9:bf9dfba36635106d6a73ccc01e28b762da60e066
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    89dd546f2de0a9d6d664f58d86097eb97baba567
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    extra:       branch=default
    description:
    Modify, add, remove, rename

    files:
    M third
    A b
    A fifth
      fourth
    R a
    R fourth"""
sh % "hg log -T status -C -r 10 --quiet" == "10:0f9759ec227a"
sh % "hg '--color=debug' log -T status -r 10" == r"""
    [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
    [log.tag|tag:         tip]
    [log.user|user:        test]
    [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
    [log.summary|summary:     Modify, add, remove, rename]
    [ui.note log.files|files:]
    [status.modified|M third]
    [status.added|A b]
    [status.added|A fifth]
    [status.removed|R a]
    [status.removed|R fourth]"""
sh % "hg '--color=debug' log -T status -C -r 10" == r"""
    [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
    [log.tag|tag:         tip]
    [log.user|user:        test]
    [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
    [log.summary|summary:     Modify, add, remove, rename]
    [ui.note log.files|files:]
    [status.modified|M third]
    [status.added|A b]
    [status.added|A fifth]
    [status.copied|  fourth]
    [status.removed|R a]
    [status.removed|R fourth]"""
sh % "hg '--color=debug' log -T status -C -r 10 -v" == r"""
    [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
    [log.tag|tag:         tip]
    [log.user|user:        test]
    [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
    [ui.note log.description|description:]
    [ui.note log.description|Modify, add, remove, rename]

    [ui.note log.files|files:]
    [status.modified|M third]
    [status.added|A b]
    [status.added|A fifth]
    [status.copied|  fourth]
    [status.removed|R a]
    [status.removed|R fourth]"""
sh % "hg '--color=debug' log -T status -C -r 10 --debug" == r"""
    [log.changeset changeset.secret|changeset:   10:0f9759ec227a4859c2014a345cd8a859022b7c6c]
    [log.tag|tag:         tip]
    [log.phase|phase:       secret]
    [log.parent changeset.secret|parent:      9:bf9dfba36635106d6a73ccc01e28b762da60e066]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    89dd546f2de0a9d6d664f58d86097eb97baba567]
    [log.user|user:        test]
    [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|Modify, add, remove, rename]

    [ui.note log.files|files:]
    [status.modified|M third]
    [status.added|A b]
    [status.added|A fifth]
    [status.copied|  fourth]
    [status.removed|R a]
    [status.removed|R fourth]"""
sh % "hg '--color=debug' log -T status -C -r 10 --quiet" == "[log.node|10:0f9759ec227a]"

# Check the bisect template

sh % "hg bisect -g 1"
sh % "hg bisect -b 3 --noupdate" == "Testing changeset 2:97054abb4ab8 (2 changesets remaining, ~1 tests)"
sh % "hg log -T bisect -r '0:4'" == r"""
    changeset:   0:1e4e1b8f71e0
    bisect:      good (implicit)
    user:        User Name <user@hostname>
    date:        Mon Jan 12 13:46:40 1970 +0000
    summary:     line 1

    changeset:   1:b608e9d1a3f0
    bisect:      good
    user:        A. N. Other <other@place>
    date:        Tue Jan 13 17:33:20 1970 +0000
    summary:     other 1

    changeset:   2:97054abb4ab8
    bisect:      untested
    user:        other@place
    date:        Wed Jan 14 21:20:00 1970 +0000
    summary:     no person

    changeset:   3:10e46f2dcbf4
    bisect:      bad
    user:        person
    date:        Fri Jan 16 01:06:40 1970 +0000
    summary:     no user, no domain

    changeset:   4:07fa1db10648
    bisect:      bad (implicit)
    bookmark:    foo
    user:        person
    date:        Sat Jan 17 04:53:20 1970 +0000
    summary:     new branch"""
sh % "hg log --debug -T bisect -r '0:4'" == r"""
    changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f
    bisect:      good (implicit)
    phase:       public
    parent:      -1:0000000000000000000000000000000000000000
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
    user:        User Name <user@hostname>
    date:        Mon Jan 12 13:46:40 1970 +0000
    files+:      a
    extra:       branch=default
    description:
    line 1
    line 2


    changeset:   1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    bisect:      good
    phase:       public
    parent:      0:1e4e1b8f71e05681d422154f5421e385fec3454f
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    4e8d705b1e53e3f9375e0e60dc7b525d8211fe55
    user:        A. N. Other <other@place>
    date:        Tue Jan 13 17:33:20 1970 +0000
    files+:      b
    extra:       branch=default
    description:
    other 1
    other 2

    other 3


    changeset:   2:97054abb4ab824450e9164180baf491ae0078465
    bisect:      untested
    phase:       public
    parent:      1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    6e0e82995c35d0d57a52aca8da4e56139e06b4b1
    user:        other@place
    date:        Wed Jan 14 21:20:00 1970 +0000
    files+:      c
    extra:       branch=default
    description:
    no person


    changeset:   3:10e46f2dcbf4823578cf180f33ecf0b957964c47
    bisect:      bad
    phase:       public
    parent:      2:97054abb4ab824450e9164180baf491ae0078465
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc
    user:        person
    date:        Fri Jan 16 01:06:40 1970 +0000
    files:       c
    extra:       branch=default
    description:
    no user, no domain


    changeset:   4:07fa1db1064879a32157227401eb44b322ae53ce
    bisect:      bad (implicit)
    bookmark:    foo
    phase:       draft
    parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47
    parent:      -1:0000000000000000000000000000000000000000
    manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc
    user:        person
    date:        Sat Jan 17 04:53:20 1970 +0000
    extra:       branch=default
    description:
    new branch"""
sh % "hg log -v -T bisect -r '0:4'" == r"""
    changeset:   0:1e4e1b8f71e0
    bisect:      good (implicit)
    user:        User Name <user@hostname>
    date:        Mon Jan 12 13:46:40 1970 +0000
    files:       a
    description:
    line 1
    line 2


    changeset:   1:b608e9d1a3f0
    bisect:      good
    user:        A. N. Other <other@place>
    date:        Tue Jan 13 17:33:20 1970 +0000
    files:       b
    description:
    other 1
    other 2

    other 3


    changeset:   2:97054abb4ab8
    bisect:      untested
    user:        other@place
    date:        Wed Jan 14 21:20:00 1970 +0000
    files:       c
    description:
    no person


    changeset:   3:10e46f2dcbf4
    bisect:      bad
    user:        person
    date:        Fri Jan 16 01:06:40 1970 +0000
    files:       c
    description:
    no user, no domain


    changeset:   4:07fa1db10648
    bisect:      bad (implicit)
    bookmark:    foo
    user:        person
    date:        Sat Jan 17 04:53:20 1970 +0000
    description:
    new branch"""
sh % "hg '--color=debug' log -T bisect -r '0:4'" == r"""
    [log.changeset changeset.public|changeset:   0:1e4e1b8f71e0]
    [log.bisect bisect.good|bisect:      good (implicit)]
    [log.user|user:        User Name <user@hostname>]
    [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
    [log.summary|summary:     line 1]

    [log.changeset changeset.public|changeset:   1:b608e9d1a3f0]
    [log.bisect bisect.good|bisect:      good]
    [log.user|user:        A. N. Other <other@place>]
    [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
    [log.summary|summary:     other 1]

    [log.changeset changeset.public|changeset:   2:97054abb4ab8]
    [log.bisect bisect.untested|bisect:      untested]
    [log.user|user:        other@place]
    [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
    [log.summary|summary:     no person]

    [log.changeset changeset.public|changeset:   3:10e46f2dcbf4]
    [log.bisect bisect.bad|bisect:      bad]
    [log.user|user:        person]
    [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
    [log.summary|summary:     no user, no domain]

    [log.changeset changeset.draft|changeset:   4:07fa1db10648]
    [log.bisect bisect.bad|bisect:      bad (implicit)]
    [log.bookmark|bookmark:    foo]
    [log.user|user:        person]
    [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
    [log.summary|summary:     new branch]"""
sh % "hg '--color=debug' log --debug -T bisect -r '0:4'" == r"""
    [log.changeset changeset.public|changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f]
    [log.bisect bisect.good|bisect:      good (implicit)]
    [log.phase|phase:       public]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0]
    [log.user|user:        User Name <user@hostname>]
    [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
    [ui.debug log.files|files+:      a]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|line 1
    line 2]


    [log.changeset changeset.public|changeset:   1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965]
    [log.bisect bisect.good|bisect:      good]
    [log.phase|phase:       public]
    [log.parent changeset.public|parent:      0:1e4e1b8f71e05681d422154f5421e385fec3454f]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    4e8d705b1e53e3f9375e0e60dc7b525d8211fe55]
    [log.user|user:        A. N. Other <other@place>]
    [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
    [ui.debug log.files|files+:      b]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|other 1
    other 2

    other 3]


    [log.changeset changeset.public|changeset:   2:97054abb4ab824450e9164180baf491ae0078465]
    [log.bisect bisect.untested|bisect:      untested]
    [log.phase|phase:       public]
    [log.parent changeset.public|parent:      1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    6e0e82995c35d0d57a52aca8da4e56139e06b4b1]
    [log.user|user:        other@place]
    [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
    [ui.debug log.files|files+:      c]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|no person]


    [log.changeset changeset.public|changeset:   3:10e46f2dcbf4823578cf180f33ecf0b957964c47]
    [log.bisect bisect.bad|bisect:      bad]
    [log.phase|phase:       public]
    [log.parent changeset.public|parent:      2:97054abb4ab824450e9164180baf491ae0078465]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc]
    [log.user|user:        person]
    [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
    [ui.debug log.files|files:       c]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|no user, no domain]


    [log.changeset changeset.draft|changeset:   4:07fa1db1064879a32157227401eb44b322ae53ce]
    [log.bisect bisect.bad|bisect:      bad (implicit)]
    [log.bookmark|bookmark:    foo]
    [log.phase|phase:       draft]
    [log.parent changeset.public|parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47]
    [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
    [ui.debug log.manifest|manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc]
    [log.user|user:        person]
    [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
    [ui.debug log.extra|extra:       branch=default]
    [ui.note log.description|description:]
    [ui.note log.description|new branch]"""
sh % "hg '--color=debug' log -v -T bisect -r '0:4'" == r"""
    [log.changeset changeset.public|changeset:   0:1e4e1b8f71e0]
    [log.bisect bisect.good|bisect:      good (implicit)]
    [log.user|user:        User Name <user@hostname>]
    [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
    [ui.note log.files|files:       a]
    [ui.note log.description|description:]
    [ui.note log.description|line 1
    line 2]


    [log.changeset changeset.public|changeset:   1:b608e9d1a3f0]
    [log.bisect bisect.good|bisect:      good]
    [log.user|user:        A. N. Other <other@place>]
    [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
    [ui.note log.files|files:       b]
    [ui.note log.description|description:]
    [ui.note log.description|other 1
    other 2

    other 3]


    [log.changeset changeset.public|changeset:   2:97054abb4ab8]
    [log.bisect bisect.untested|bisect:      untested]
    [log.user|user:        other@place]
    [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
    [ui.note log.files|files:       c]
    [ui.note log.description|description:]
    [ui.note log.description|no person]


    [log.changeset changeset.public|changeset:   3:10e46f2dcbf4]
    [log.bisect bisect.bad|bisect:      bad]
    [log.user|user:        person]
    [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
    [ui.note log.files|files:       c]
    [ui.note log.description|description:]
    [ui.note log.description|no user, no domain]


    [log.changeset changeset.draft|changeset:   4:07fa1db10648]
    [log.bisect bisect.bad|bisect:      bad (implicit)]
    [log.bookmark|bookmark:    foo]
    [log.user|user:        person]
    [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
    [ui.note log.description|description:]
    [ui.note log.description|new branch]"""
sh % "hg bisect --reset"

# Error on syntax:

sh % "echo 'x = \"f'" >> "t"
sh % "hg log" == r"""
    hg: parse error at t:3: unmatched quotes
    [255]"""

sh % "hg log -T '{date'" == r"""
    hg: parse error at 1: unterminated template expansion
    ({date
     ^ here)
    [255]"""

# Behind the scenes, this will throw TypeError

sh % "hg log -l 3 --template '{date|obfuscate}\\n'" == r"""
    abort: template filter 'obfuscate' is not compatible with keyword 'date'
    [255]"""

# Behind the scenes, this will throw a ValueError

sh % "hg log -l 3 --template 'line: {desc|shortdate}\\n'" == r"""
    abort: template filter 'shortdate' is not compatible with keyword 'desc'
    [255]"""

# Behind the scenes, this will throw AttributeError

sh % "hg log -l 3 --template 'line: {date|escape}\\n'" == r"""
    abort: template filter 'escape' is not compatible with keyword 'date'
    [255]"""

sh % "hg log -l 3 --template 'line: {extras|localdate}\\n'" == r"""
    hg: parse error: localdate expects a date information
    [255]"""

# Behind the scenes, this will throw ValueError

sh % "hg tip --template '{author|email|date}\\n'" == r"""
    hg: parse error: date expects a date information
    [255]"""

sh % "hg tip -T '{author|email|shortdate}\\n'" == r"""
    abort: template filter 'shortdate' is not compatible with keyword 'author'
    [255]"""

sh % "hg tip -T '{get(extras, \"branch\")|shortdate}\\n'" == r"""
    abort: incompatible use of template filter 'shortdate'
    [255]"""

# Error in nested template:

sh % "hg log -T '{\"date'" == r"""
    hg: parse error at 2: unterminated string
    ({"date
      ^ here)
    [255]"""

sh % "hg log -T '{\"foo{date|?}\"}'" == r"""
    hg: parse error at 11: syntax error
    ({"foo{date|?}"}
               ^ here)
    [255]"""

# Thrown an error if a template function doesn't exist

sh % "hg tip --template '{foo()}\\n'" == r"""
    hg: parse error: unknown function 'foo'
    [255]"""

# Pass generator object created by template function to filter

sh % "hg log -l 1 --template '{if(author, author)|user}\\n'" == "test"

# Test index keyword:

sh % "hg log -l 2 -T '{index + 10}{files % \" {index}:{file}\"}\\n'" == r"""
    10 0:a 1:b 2:fifth 3:fourth 4:third
    11 0:a"""

# Test diff function:

sh % "hg diff -c 8" == r"""
    diff -r 29114dbae42b -r 95c24699272e fourth
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +second
    diff -r 29114dbae42b -r 95c24699272e second
    --- a/second	Mon Jan 12 13:46:40 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -second
    diff -r 29114dbae42b -r 95c24699272e third
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/third	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +third"""

sh % "hg log -r 8 -T '{diff()}'" == r"""
    diff -r 29114dbae42b -r 95c24699272e fourth
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +second
    diff -r 29114dbae42b -r 95c24699272e second
    --- a/second	Mon Jan 12 13:46:40 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -second
    diff -r 29114dbae42b -r 95c24699272e third
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/third	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +third"""

sh % "hg log -r 8 -T '{diff('\\''glob:f*'\\'')}'" == r"""
    diff -r 29114dbae42b -r 95c24699272e fourth
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +second"""

sh % "hg log -r 8 -T '{diff('\\'''\\'', '\\''glob:f*'\\'')}'" == r"""
    diff -r 29114dbae42b -r 95c24699272e second
    --- a/second	Mon Jan 12 13:46:40 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -second
    diff -r 29114dbae42b -r 95c24699272e third
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/third	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +third"""

sh % "hg log -r 8 -T '{diff('\\''FOURTH'\\''|lower)}'" == r"""
    diff -r 29114dbae42b -r 95c24699272e fourth
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
    @@ -0,0 +1,1 @@
    +second"""

# ui verbosity:

sh % "hg log -l1 -T '{verbosity}\\n'"
sh % "hg log -l1 -T '{verbosity}\\n' --debug" == "debug"
sh % "hg log -l1 -T '{verbosity}\\n' --quiet" == "quiet"
sh % "hg log -l1 -T '{verbosity}\\n' --verbose" == "verbose"

sh % "cd .."


# latesttag:

sh % "hg init latesttag"
sh % "cd latesttag"

sh % "echo a" > "file"
sh % "hg ci -Am a -d '0 0'" == "adding file"

sh % "echo b" >> "file"
sh % "hg ci -m b -d '1 0'"

sh % "echo c" >> "head1"
sh % "hg ci -Am h1c -d '2 0'" == "adding head1"

sh % "hg update -q 1"
sh % "echo d" >> "head2"
sh % "hg ci -Am h2d -d '3 0'" == "adding head2"

sh % "echo e" >> "head2"
sh % "hg ci -m h2e -d '4 0'"

sh % "hg merge -q"
sh % "hg ci -m merge -d '5 -3600'"

# No tag set:

sh % "hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\\n'" == r"""
    @    5: null+5
    |\
    | o  4: null+4
    | |
    | o  3: null+3
    | |
    o |  2: null+3
    |/
    o  1: null+2
    |
    o  0: null+1"""

# One common tag: longest path wins for {latesttagdistance}:

sh % "hg tag -r 1 -m t1 -d '6 0' t1"
sh % "hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\\n'" == r"""
    @  6: t1+4
    |
    o    5: t1+3
    |\
    | o  4: t1+2
    | |
    | o  3: t1+1
    | |
    o |  2: t1+1
    |/
    o  1: t1+0
    |
    o  0: null+1"""

# One ancestor tag: closest wins:

sh % "hg tag -r 2 -m t2 -d '7 0' t2"
sh % "hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\\n'" == r"""
    @  7: t2+3
    |
    o  6: t2+2
    |
    o    5: t2+1
    |\
    | o  4: t1+2
    | |
    | o  3: t1+1
    | |
    o |  2: t2+0
    |/
    o  1: t1+0
    |
    o  0: null+1"""

# Two branch tags: more recent wins if same number of changes:

sh % "hg tag -r 3 -m t3 -d '8 0' t3"
sh % "hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\\n'" == r"""
    @  8: t3+5
    |
    o  7: t3+4
    |
    o  6: t3+3
    |
    o    5: t3+2
    |\
    | o  4: t3+1
    | |
    | o  3: t3+0
    | |
    o |  2: t2+0
    |/
    o  1: t1+0
    |
    o  0: null+1"""

# Two branch tags: fewest changes wins:

sh % "hg tag -r 4 -m t4 -d '4 0' t4"
sh % "hg log -G --template '{rev}: {latesttag % '\\''{tag}+{distance},{changes} '\\''}\\n'" == r"""
    @  9: t4+5,6
    |
    o  8: t4+4,5
    |
    o  7: t4+3,4
    |
    o  6: t4+2,3
    |
    o    5: t4+1,2
    |\
    | o  4: t4+0,0
    | |
    | o  3: t3+0,0
    | |
    o |  2: t2+0,0
    |/
    o  1: t1+0,0
    |
    o  0: null+1,1"""

# Merged tag overrides:

sh % "hg tag -r 5 -m t5 -d '9 0' t5"
sh % "hg tag -r 3 -m at3 -d '10 0' at3"
sh % "hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\\n'" == r"""
    @  11: t5+6
    |
    o  10: t5+5
    |
    o  9: t5+4
    |
    o  8: t5+3
    |
    o  7: t5+2
    |
    o  6: t5+1
    |
    o    5: t5+0
    |\
    | o  4: t4+0
    | |
    | o  3: at3:t3+0
    | |
    o |  2: t2+0
    |/
    o  1: t1+0
    |
    o  0: null+1"""

sh % "hg log -G --template '{rev}: {latesttag % '\\''{tag}+{distance},{changes} '\\''}\\n'" == r"""
    @  11: t5+6,6
    |
    o  10: t5+5,5
    |
    o  9: t5+4,4
    |
    o  8: t5+3,3
    |
    o  7: t5+2,2
    |
    o  6: t5+1,1
    |
    o    5: t5+0,0
    |\
    | o  4: t4+0,0
    | |
    | o  3: at3+0,0 t3+0,0
    | |
    o |  2: t2+0,0
    |/
    o  1: t1+0,0
    |
    o  0: null+1,1"""

sh % "hg log -G --template '{rev}: {latesttag('\\''re:^t[13]$'\\'') % '\\''{tag}, C: {changes}, D: {distance}'\\''}\\n'" == r"""
    @  11: t3, C: 9, D: 8
    |
    o  10: t3, C: 8, D: 7
    |
    o  9: t3, C: 7, D: 6
    |
    o  8: t3, C: 6, D: 5
    |
    o  7: t3, C: 5, D: 4
    |
    o  6: t3, C: 4, D: 3
    |
    o    5: t3, C: 3, D: 2
    |\
    | o  4: t3, C: 1, D: 1
    | |
    | o  3: t3, C: 0, D: 0
    | |
    o |  2: t1, C: 1, D: 1
    |/
    o  1: t1, C: 0, D: 0
    |
    o  0: null, C: 1, D: 1"""

sh % "cd .."


# Style path expansion: issue1948 - ui.style option doesn't work on OSX
# if it is a relative path

sh % "mkdir -p home/styles"

sh % "cat" << r"""
changeset = 'test {rev}:{node|short}\n'
""" > "home/styles/teststyle"

sh % "'HOME=`pwd`/home;' export HOME"

sh % "cat" << r"""
[ui]
style = ~/styles/teststyle
""" > "latesttag/.hg/hgrc"

sh % "hg -R latesttag tip" == "test 11:97e5943b523a"

# Test recursive showlist template (issue1989):

sh % "cat" << r"""
changeset = '{file_mods}{manifest}{extras}'
file_mod  = 'M|{author|person}\n'
manifest = '{rev},{author}\n'
extra = '{key}: {author}\n'
""" > "style1989"

sh % "hg -R latesttag log -r tip '--style=style1989'" == r"""
    M|test
    11,test
    branch: test"""

# Test new-style inline templating:

sh % "hg log -R latesttag -r tip --template 'modified files: {file_mods % \" {file}\\n\"}\\n'" == "modified files:  .hgtags"

sh % "hg log -R latesttag -r tip -T '{rev % \"a\"}\\n'" == r"""
    hg: parse error: keyword 'rev' is not iterable
    [255]"""
sh % 'hg log -R latesttag -r tip -T \'{get(extras, "unknown") % "a"}\\n\'' == r"""
    hg: parse error: None is not iterable
    [255]"""

# Test new-style inline templating of non-list/dict type:

sh % "hg log -R latesttag -r tip -T '{manifest}\\n'" == "11:2bc6e9006ce2"
sh % "hg log -R latesttag -r tip -T 'string length: {manifest|count}\\n'" == "string length: 15"
sh % "hg log -R latesttag -r tip -T '{manifest % \"{rev}:{node}\"}\\n'" == "11:2bc6e9006ce29882383a22d39fd1f4e66dd3e2fc"

sh % 'hg log -R latesttag -r tip -T \'{get(extras, "branch") % "{key}: {value}\\n"}\'' == "branch: default"
sh % 'hg log -R latesttag -r tip -T \'{get(extras, "unknown") % "{key}\\n"}\'' == r"""
    hg: parse error: None is not iterable
    [255]"""
sh % "hg log -R latesttag -r tip -T '{min(extras) % \"{key}: {value}\\n\"}'" == "branch: default"
sh % 'hg log -R latesttag -l1 -T \'{min(revset("0:9")) % "{rev}:{node|short}\\n"}\'' == "0:ce3cec86e6c2"
sh % 'hg log -R latesttag -l1 -T \'{max(revset("0:9")) % "{rev}:{node|short}\\n"}\'' == "9:fbc7cd862e9c"

# Test manifest/get() can be join()-ed as before, though it's silly:

sh % "hg log -R latesttag -r tip -T '{join(manifest, \"\")}\\n'" == "11:2bc6e9006ce2"
sh % 'hg log -R latesttag -r tip -T \'{join(get(extras, "branch"), "")}\\n\'' == "default"

# Test min/max of integers

sh % "hg log -R latesttag -l1 -T '{min(revset(\"9:10\"))}\\n'" == "9"
sh % "hg log -R latesttag -l1 -T '{max(revset(\"9:10\"))}\\n'" == "10"

# Test dot operator precedence:

sh % "hg debugtemplate -R latesttag -r0 -v '{manifest.node|short}\\n'" == r"""
    (template
      (|
        (.
          (symbol 'manifest')
          (symbol 'node'))
        (symbol 'short'))
      (string '\n'))
    89f4071fec70"""

#  (the following examples are invalid, but seem natural in parsing POV)

sh % "hg debugtemplate -R latesttag -r0 -v '{foo|bar.baz}\\n' '2>' /dev/null" == r"""
    (template
      (|
        (symbol 'foo')
        (.
          (symbol 'bar')
          (symbol 'baz')))
      (string '\n'))
    [255]"""
sh % "hg debugtemplate -R latesttag -r0 -v '{foo.bar()}\\n' '2>' /dev/null" == r"""
    (template
      (.
        (symbol 'foo')
        (func
          (symbol 'bar')
          None))
      (string '\n'))
    [255]"""

# Test evaluation of dot operator:

sh % "hg log -R latesttag -l1 -T '{min(revset(\"0:9\")).node}\\n'" == "ce3cec86e6c26bd9bdfc590a6b92abc9680f1796"
sh % "hg log -R latesttag -r0 -T '{extras.branch}\\n'" == "default"

sh % "hg log -R latesttag -l1 -T '{author.invalid}\\n'" == r"""
    hg: parse error: keyword 'author' has no member
    [255]"""
sh % "hg log -R latesttag -l1 -T '{min(\"abc\").invalid}\\n'" == r"""
    hg: parse error: 'a' has no member
    [255]"""

# Test the sub function of templating for expansion:

sh % 'hg log -R latesttag -r 10 --template \'{sub("[0-9]", "x", "{rev}")}\\n\'' == "xx"

sh % 'hg log -R latesttag -r 10 -T \'{sub("[", "x", rev)}\\n\'' == r"""
    hg: parse error: sub got an invalid pattern: [
    [255]"""
sh % 'hg log -R latesttag -r 10 -T \'{sub("[0-9]", r"\\1", rev)}\\n\'' == r"""
    hg: parse error: sub got an invalid replacement: \1
    [255]"""

# Test the strip function with chars specified:

sh % "hg log -R latesttag --template '{desc}\\n'" == r"""
    at3
    t5
    t4
    t3
    t2
    t1
    merge
    h2e
    h2d
    h1c
    b
    a"""

sh % "hg log -R latesttag --template '{strip(desc, \"te\")}\\n'" == r"""
    at3
    5
    4
    3
    2
    1
    merg
    h2
    h2d
    h1c
    b
    a"""

# Test date format:

sh % "hg log -R latesttag --template 'date: {date(date, \"%y %m %d %S %z\")}\\n'" == r"""
    date: 70 01 01 10 +0000
    date: 70 01 01 09 +0000
    date: 70 01 01 04 +0000
    date: 70 01 01 08 +0000
    date: 70 01 01 07 +0000
    date: 70 01 01 06 +0000
    date: 70 01 01 05 +0100
    date: 70 01 01 04 +0000
    date: 70 01 01 03 +0000
    date: 70 01 01 02 +0000
    date: 70 01 01 01 +0000
    date: 70 01 01 00 +0000"""

# Test invalid date:

sh % "hg log -R latesttag -T '{date(rev)}\\n'" == r"""
    hg: parse error: date expects a date information
    [255]"""

# Test integer literal:

sh % "hg debugtemplate -v '{(0)}\\n'" == r"""
    (template
      (group
        (integer '0'))
      (string '\n'))
    0"""
sh % "hg debugtemplate -v '{(123)}\\n'" == r"""
    (template
      (group
        (integer '123'))
      (string '\n'))
    123"""
sh % "hg debugtemplate -v '{(-4)}\\n'" == r"""
    (template
      (group
        (negate
          (integer '4')))
      (string '\n'))
    -4"""
sh % "hg debugtemplate '{(-)}\\n'" == r"""
    hg: parse error at 3: not a prefix: )
    ({(-)}\n
       ^ here)
    [255]"""
sh % "hg debugtemplate '{(-a)}\\n'" == r"""
    hg: parse error: negation needs an integer argument
    [255]"""

# top-level integer literal is interpreted as symbol (i.e. variable name):

sh % "hg debugtemplate -D '1=one' -v '{1}\\n'" == r"""
    (template
      (integer '1')
      (string '\n'))
    one"""
sh % "hg debugtemplate -D '1=one' -v '{if(\"t\", \"{1}\")}\\n'" == r"""
    (template
      (func
        (symbol 'if')
        (list
          (string 't')
          (template
            (integer '1'))))
      (string '\n'))
    one"""
sh % "hg debugtemplate -D '1=one' -v '{1|stringify}\\n'" == r"""
    (template
      (|
        (integer '1')
        (symbol 'stringify'))
      (string '\n'))
    one"""

# unless explicit symbol is expected:

sh % "hg log -Ra -r0 -T '{desc|1}\\n'" == r"""
    hg: parse error: expected a symbol, got 'integer'
    [255]"""
sh % "hg log -Ra -r0 -T '{1()}\\n'" == r"""
    hg: parse error: expected a symbol, got 'integer'
    [255]"""

# Test string literal:

sh % "hg debugtemplate -Ra -r0 -v '{\"string with no template fragment\"}\\n'" == r"""
    (template
      (string 'string with no template fragment')
      (string '\n'))
    string with no template fragment"""
sh % "hg debugtemplate -Ra -r0 -v '{\"template: {rev}\"}\\n'" == r"""
    (template
      (template
        (string 'template: ')
        (symbol 'rev'))
      (string '\n'))
    template: 0"""
sh % "hg debugtemplate -Ra -r0 -v '{r\"rawstring: {rev}\"}\\n'" == r"""
    (template
      (string 'rawstring: {rev}')
      (string '\n'))
    rawstring: {rev}"""
sh % "hg debugtemplate -Ra -r0 -v '{files % r\"rawstring: {file}\"}\\n'" == r"""
    (template
      (%
        (symbol 'files')
        (string 'rawstring: {file}'))
      (string '\n'))
    rawstring: {file}"""

# Test string escaping:

sh % "hg log -R latesttag -r 0 --template" > '\\n<>\\\\n<{if(rev, "[>\\n<>\\\\n<]")}>\\n<>\\\\n<\\n' == r"""
    >
    <>\n<[>
    <>\n<]>
    <>\n<"""

sh % "hg log -R latesttag -r 0 --config 'ui.logtemplate=>\\n<>\\\\n<{if(rev, \"[>\\n<>\\\\n<]\")}>\\n<>\\\\n<\\n'" == r"""
    >
    <>\n<[>
    <>\n<]>
    <>\n<"""

sh % "hg log -R latesttag -r 0 -T esc --config 'templates.esc=>\\n<>\\\\n<{if(rev, \"[>\\n<>\\\\n<]\")}>\\n<>\\\\n<\\n'" == r"""
    >
    <>\n<[>
    <>\n<]>
    <>\n<"""

sh % "cat" << r"""
changeset = '>\n<>\\n<{if(rev, "[>\n<>\\n<]")}>\n<>\\n<\n'
""" > "esctmpl"
sh % "hg log -R latesttag -r 0 --style ./esctmpl" == r"""
    >
    <>\n<[>
    <>\n<]>
    <>\n<"""

# Test string escaping of quotes:

sh % 'hg log -Ra -r0 -T \'{"\\""}\\n\'' == '"'
sh % 'hg log -Ra -r0 -T \'{"\\\\\\""}\\n\'' == '\\"'
sh % 'hg log -Ra -r0 -T \'{r"\\""}\\n\'' == '\\"'
sh % 'hg log -Ra -r0 -T \'{r"\\\\\\""}\\n\'' == '\\\\\\"'


sh % 'hg log -Ra -r0 -T \'{"\\""}\\n\'' == '"'
sh % 'hg log -Ra -r0 -T \'{"\\\\\\""}\\n\'' == '\\"'
sh % 'hg log -Ra -r0 -T \'{r"\\""}\\n\'' == '\\"'
sh % 'hg log -Ra -r0 -T \'{r"\\\\\\""}\\n\'' == '\\\\\\"'

# Test exception in quoted template. single backslash before quotation mark is
# stripped before parsing:

sh % "cat" << r"""
changeset = "\" \\" \\\" \\\\" {files % \"{file}\"}\n"
""" > "escquotetmpl"
sh % "cd latesttag"
sh % "hg log -r 2 --style ../escquotetmpl" == '" \\" \\" \\\\" head1'

sh % 'hg log -r 2 -T esc --config \'templates.esc="{\\"valid\\"}\\n"\'' == "valid"
sh % "hg log -r 2 -T esc --config 'templates.esc='\\''{\\'\\''valid\\'\\''}\\n'\\'''" == "valid"

# Test compatibility with 2.9.2-3.4 of escaped quoted strings in nested
# _evalifliteral() templates (issue4733):

sh % 'hg log -r 2 -T \'{if(rev, "\\"{rev}")}\\n\'' == '"2'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\"\\\\\\"{rev}\\")}")}\\n\'' == '"2'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\"{if(rev, \\\\\\"\\\\\\\\\\\\\\"{rev}\\\\\\")}\\")}")}\\n\'' == '"2'

sh % 'hg log -r 2 -T \'{if(rev, "\\\\\\"")}\\n\'' == '\\"'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\"\\\\\\\\\\\\\\"\\")}")}\\n\'' == '\\"'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\"{if(rev, \\\\\\"\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"\\\\\\")}\\")}")}\\n\'' == '\\"'

sh % 'hg log -r 2 -T \'{if(rev, r"\\\\\\"")}\\n\'' == '\\\\\\"'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, r\\"\\\\\\\\\\\\\\"\\")}")}\\n\'' == '\\\\\\"'
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\"{if(rev, r\\\\\\"\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"\\\\\\")}\\")}")}\\n\'' == '\\\\\\"'

# escaped single quotes and errors:

sh % "hg log -r 2 -T '{if(rev, '\\''{if(rev, \\'\\''foo\\'\\'')}'\\'')}\\n'" == "foo"
sh % "hg log -r 2 -T '{if(rev, '\\''{if(rev, r\\'\\''foo\\'\\'')}'\\'')}\\n'" == "foo"
sh % 'hg log -r 2 -T \'{if(rev, "{if(rev, \\")}")}\\n\'' == r"""
    hg: parse error at 21: unterminated string
    ({if(rev, "{if(rev, \")}")}\n
                         ^ here)
    [255]"""
sh % 'hg log -r 2 -T \'{if(rev, \\"\\\\"")}\\n\'' == r"""
    hg: parse error: trailing \ in string
    [255]"""
sh % 'hg log -r 2 -T \'{if(rev, r\\"\\\\"")}\\n\'' == r"""
    hg: parse error: trailing \ in string
    [255]"""

sh % "cd .."

# Test leading backslashes:

sh % "cd latesttag"
sh % "hg log -r 2 -T '\\{rev} {files % \"\\{file}\"}\\n'" == "{rev} {file}"
sh % "hg log -r 2 -T '\\\\{rev} {files % \"\\\\{file}\"}\\n'" == "\\2 \\head1"
sh % "hg log -r 2 -T '\\\\\\{rev} {files % \"\\\\\\{file}\"}\\n'" == "\\{rev} \\{file}"
sh % "cd .."

# Test leading backslashes in "if" expression (issue4714):

sh % "cd latesttag"
sh % 'hg log -r 2 -T \'{if("1", "\\{rev}")} {if("1", r"\\{rev}")}\\n\'' == "{rev} \\{rev}"
sh % 'hg log -r 2 -T \'{if("1", "\\\\{rev}")} {if("1", r"\\\\{rev}")}\\n\'' == "\\2 \\\\{rev}"
sh % 'hg log -r 2 -T \'{if("1", "\\\\\\{rev}")} {if("1", r"\\\\\\{rev}")}\\n\'' == "\\{rev} \\\\\\{rev}"
sh % "cd .."

# "string-escape"-ed "\x5c\x786e" becomes r"\x6e" (once) or r"n" (twice)

sh % 'hg log -R a -r 0 --template \'{if("1", "\\x5c\\x786e", "NG")}\\n\'' == "\\x6e"
sh % 'hg log -R a -r 0 --template \'{if("1", r"\\x5c\\x786e", "NG")}\\n\'' == "\\x5c\\x786e"
sh % 'hg log -R a -r 0 --template \'{if("", "NG", "\\x5c\\x786e")}\\n\'' == "\\x6e"
sh % 'hg log -R a -r 0 --template \'{if("", "NG", r"\\x5c\\x786e")}\\n\'' == "\\x5c\\x786e"

sh % 'hg log -R a -r 2 --template \'{ifeq("no perso\\x6e", desc, "\\x5c\\x786e", "NG")}\\n\'' == "\\x6e"
sh % 'hg log -R a -r 2 --template \'{ifeq(r"no perso\\x6e", desc, "NG", r"\\x5c\\x786e")}\\n\'' == "\\x5c\\x786e"
sh % 'hg log -R a -r 2 --template \'{ifeq(desc, "no perso\\x6e", "\\x5c\\x786e", "NG")}\\n\'' == "\\x6e"
sh % 'hg log -R a -r 2 --template \'{ifeq(desc, r"no perso\\x6e", "NG", r"\\x5c\\x786e")}\\n\'' == "\\x5c\\x786e"

sh % "hg log -R a -r 8 --template '{join(files, \"\\n\")}\\n'" == r"""
    fourth
    second
    third"""
sh % "hg log -R a -r 8 --template '{join(files, r\"\\n\")}\\n'" == "fourth\\nsecond\\nthird"

sh % 'hg log -R a -r 2 --template \'{rstdoc("1st\\n\\n2nd", "htm\\x6c")}\'' == r"""
    <p>
    1st
    </p>
    <p>
    2nd
    </p>"""
sh % 'hg log -R a -r 2 --template \'{rstdoc(r"1st\\n\\n2nd", "html")}\'' == r"""
    <p>
    1st\n\n2nd
    </p>"""
sh % 'hg log -R a -r 2 --template \'{rstdoc("1st\\n\\n2nd", r"htm\\x6c")}\'' == r"""
    1st

    2nd"""

sh % "hg log -R a -r 2 --template '{strip(desc, \"\\x6e\")}\\n'" == "o perso"
sh % "hg log -R a -r 2 --template '{strip(desc, r\"\\x6e\")}\\n'" == "no person"
sh % 'hg log -R a -r 2 --template \'{strip("no perso\\x6e", "\\x6e")}\\n\'' == "o perso"
sh % 'hg log -R a -r 2 --template \'{strip(r"no perso\\x6e", r"\\x6e")}\\n\'' == "no perso"

sh % 'hg log -R a -r 2 --template \'{sub("\\\\x6e", "\\x2d", desc)}\\n\'' == "-o perso-"
sh % 'hg log -R a -r 2 --template \'{sub(r"\\\\x6e", "-", desc)}\\n\'' == "no person"
sh % 'hg log -R a -r 2 --template \'{sub("n", r"\\x2d", desc)}\\n\'' == "\\x2do perso\\x2d"
sh % 'hg log -R a -r 2 --template \'{sub("n", "\\x2d", "no perso\\x6e")}\\n\'' == "-o perso-"
sh % 'hg log -R a -r 2 --template \'{sub("n", r"\\x2d", r"no perso\\x6e")}\\n\'' == "\\x2do perso\\x6e"

sh % "hg log -R a -r 8 --template '{files % \"{file}\\n\"}'" == r"""
    fourth
    second
    third"""

# Test string escaping in nested expression:

sh % 'hg log -R a -r 8 --template \'{ifeq(r"\\x6e", if("1", "\\x5c\\x786e"), join(files, "\\x5c\\x786e"))}\\n\'' == "fourth\\x6esecond\\x6ethird"
sh % 'hg log -R a -r 8 --template \'{ifeq(if("1", r"\\x6e"), "\\x5c\\x786e", join(files, "\\x5c\\x786e"))}\\n\'' == "fourth\\x6esecond\\x6ethird"

sh % 'hg log -R a -r 8 --template \'{join(files, ifeq(branch, "default", "\\x5c\\x786e"))}\\n\'' == "fourth\\x6esecond\\x6ethird"
sh % 'hg log -R a -r 8 --template \'{join(files, ifeq(branch, "default", r"\\x5c\\x786e"))}\\n\'' == "fourth\\x5c\\x786esecond\\x5c\\x786ethird"

sh % 'hg log -R a -r \'3:4\' --template \'{rev}:{sub(if("1", "\\x6e"), ifeq(branch, "foo", r"\\x5c\\x786e", "\\x5c\\x786e"), desc)}\\n\'' == r"""
    3:\x6eo user, \x6eo domai\x6e
    4:\x6eew bra\x6ech"""

# Test quotes in nested expression are evaluated just like a $(command)
# substitution in POSIX shells:

sh % 'hg log -R a -r 8 -T \'{"{"{rev}:{node|short}"}"}\\n\'' == "8:95c24699272e"
sh % 'hg log -R a -r 8 -T \'{"{"\\{{rev}} \\"{node|short}\\""}"}\\n\'' == '{8} "95c24699272e"'

# Test recursive evaluation:

sh % "hg init r"
sh % "cd r"
sh % "echo a" > "a"
sh % "hg ci -Am '{rev}'" == "adding a"
sh % "hg log -r 0 --template '{if(rev, desc)}\\n'" == "{rev}"
sh % "hg log -r 0 --template '{if(rev, \"{author} {rev}\")}\\n'" == "test 0"

sh % "hg bookmark -q 'text.{rev}'"
sh % "echo aa" >> "aa"
sh % "hg ci -u '{node|short}' -m 'desc to be wrapped desc to be wrapped'"

sh % "hg log -l1 --template '{fill(desc, \"20\", author, bookmarks)}'" == r"""
    {node|short}desc to
    text.{rev}be wrapped
    text.{rev}desc to be
    text.{rev}wrapped (no-eol)"""
sh % 'hg log -l1 --template \'{fill(desc, "20", "{node|short}:", "text.{rev}:")}\'' == r"""
    ea4c0948489d:desc to
    text.1:be wrapped
    text.1:desc to be
    text.1:wrapped (no-eol)"""
sh % 'hg log -l1 -T \'{fill(desc, date, "", "")}\\n\'' == r"""
    hg: parse error: fill expects an integer width
    [255]"""

sh % "'COLUMNS=25' hg log -l1 --template '{fill(desc, termwidth, \"{node|short}:\", \"termwidth.{rev}:\")}'" == r"""
    ea4c0948489d:desc to be
    termwidth.1:wrapped desc
    termwidth.1:to be wrapped (no-eol)"""

sh % 'hg log -l 1 --template \'{sub(r"[0-9]", "-", author)}\'' == "{node|short} (no-eol)"
sh % 'hg log -l 1 --template \'{sub(r"[0-9]", "-", "{node|short}")}\'' == "ea-c-------d (no-eol)"

sh % "cat" << r"""
[extensions]
color=
[color]
mode=ansi
text.{rev} = red
text.1 = green
""" >> ".hg/hgrc"
sh % "hg log '--color=always' -l 1 --template '{label(bookmarks, \"text\\n\")}'" == "\\x1b[0;31mtext\\x1b[0m (esc)"
sh % "hg log '--color=always' -l 1 --template '{label(\"text.{rev}\", \"text\\n\")}'" == "\\x1b[0;32mtext\\x1b[0m (esc)"

# color effect can be specified without quoting:

sh % "hg log '--color=always' -l 1 --template '{label(red, \"text\\n\")}'" == "\\x1b[0;31mtext\\x1b[0m (esc)"

# color effects can be nested (issue5413)

sh % 'hg debugtemplate \'--color=always\' \'{label(red, "red{label(magenta, "ma{label(cyan, "cyan")}{label(yellow, "yellow")}genta")}")}\\n\'' == "\\x1b[0;31mred\\x1b[0;35mma\\x1b[0;36mcyan\\x1b[0m\\x1b[0;31m\\x1b[0;35m\\x1b[0;33myellow\\x1b[0m\\x1b[0;31m\\x1b[0;35mgenta\\x1b[0m (esc)"

# pad() should interact well with color codes (issue5416)

sh % "hg debugtemplate '--color=always' '{pad(label(red, \"red\"), 5, label(cyan, \"-\"))}\\n'" == "\\x1b[0;31mred\\x1b[0m\\x1b[0;36m-\\x1b[0m\\x1b[0;36m-\\x1b[0m (esc)"

# label should be no-op if color is disabled:

sh % "hg log '--color=never' -l 1 --template '{label(red, \"text\\n\")}'" == "text"
sh % "hg log --config 'extensions.color=!' -l 1 --template '{label(red, \"text\\n\")}'" == "text"

# Test dict constructor:

sh % "hg log -r 0 -T '{dict(y=node|short, x=rev)}\\n'" == "y=f7769ec2ab97 x=0"
sh % "hg log -r 0 -T '{dict(x=rev, y=node|short) % \"{key}={value}\\n\"}'" == r"""
    x=0
    y=f7769ec2ab97"""
sh % "hg log -r 0 -T '{dict(x=rev, y=node|short)|json}\\n'" == '{"x": 0, "y": "f7769ec2ab97"}'
sh % "hg log -r 0 -T '{dict()|json}\\n'" == "{}"

sh % "hg log -r 0 -T '{dict(rev, node=node|short)}\\n'" == "rev=0 node=f7769ec2ab97"
sh % "hg log -r 0 -T '{dict(rev, node|short)}\\n'" == "rev=0 node=f7769ec2ab97"

sh % "hg log -r 0 -T '{dict(rev, rev=rev)}\\n'" == r"""
    hg: parse error: duplicated dict key 'rev' inferred
    [255]"""
sh % "hg log -r 0 -T '{dict(node, node|short)}\\n'" == r"""
    hg: parse error: duplicated dict key 'node' inferred
    [255]"""
sh % "hg log -r 0 -T '{dict(1 + 2)}'" == r"""
    hg: parse error: dict key cannot be inferred
    [255]"""

sh % "hg log -r 0 -T '{dict(x=rev, x=node)}'" == r"""
    hg: parse error: dict got multiple values for keyword argument 'x'
    [255]"""

# Test get function:

sh % "hg log -r 0 --template '{get(extras, \"branch\")}\\n'" == "default"
sh % 'hg log -r 0 --template \'{get(extras, "br{"anch"}")}\\n\'' == "default"
sh % "hg log -r 0 --template '{get(files, \"should_fail\")}\\n'" == r"""
    hg: parse error: get() expects a dict as first argument
    [255]"""

# Test json filter applied to hybrid object:

sh % "hg log -r0 -T '{files|json}\\n'" == '["a"]'
sh % "hg log -r0 -T '{extras|json}\\n'" == '{"branch": "default"}'

# Test localdate(date, tz) function:

sh % "'TZ=JST-09' hg log -r0 -T '{date|localdate|isodate}\\n'" == "1970-01-01 09:00 +0900"
sh % "'TZ=JST-09' hg log -r0 -T '{localdate(date, \"UTC\")|isodate}\\n'" == "1970-01-01 00:00 +0000"
sh % "'TZ=JST-09' hg log -r0 -T '{localdate(date, \"blahUTC\")|isodate}\\n'" == r"""
    hg: parse error: localdate expects a timezone
    [255]"""
sh % "'TZ=JST-09' hg log -r0 -T '{localdate(date, \"+0200\")|isodate}\\n'" == "1970-01-01 02:00 +0200"
sh % "'TZ=JST-09' hg log -r0 -T '{localdate(date, \"0\")|isodate}\\n'" == "1970-01-01 00:00 +0000"
sh % "'TZ=JST-09' hg log -r0 -T '{localdate(date, 0)|isodate}\\n'" == "1970-01-01 00:00 +0000"
sh % "hg log -r0 -T '{localdate(date, \"invalid\")|isodate}\\n'" == r"""
    hg: parse error: localdate expects a timezone
    [255]"""
sh % "hg log -r0 -T '{localdate(date, date)|isodate}\\n'" == r"""
    hg: parse error: localdate expects a timezone
    [255]"""

# Test shortest(node) function:

sh % "echo b" > "b"
sh % "hg ci -qAm b"
sh % "hg log --template '{shortest(node)}\\n'" == r"""
    21c1
    ea4c
    f776"""
sh % "hg log --template '{shortest(node, 10)}\\n'" == r"""
    21c1b7ca5a
    ea4c094848
    f7769ec2ab"""
sh % "hg log --template '{node|shortest}\\n' -l1" == "21c1"

sh % 'hg log -r 0 -T \'{shortest(node, "1{"0"}")}\\n\'' == "f7769ec2ab"
sh % "hg log -r 0 -T '{shortest(node, \"not an int\")}\\n'" == r"""
    hg: parse error: shortest() expects an integer minlength
    [255]"""

sh % "hg log -r 'wdir()' -T '{node|shortest}\\n'" == "ffff"

sh % "cd .."

# Test shortest(node) with the repo having short hash collision:

sh % "hg init hashcollision"
sh % "cd hashcollision"
sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> ".hg/hgrc"
sh % "echo 0" > "a"
sh % "hg ci -qAm 0"
sh % "for i in 17 129 248 242 480 580 617 1057 2857 '4025;' do" == r"""
    >   hg up -q 0
    >   echo $i > a
    >   hg ci -qm $i
    > done"""
sh % "hg up -q null"
sh % "hg log '-r0:' -T '{rev}:{node}\\n'" == r"""
    0:b4e73ffab476aa0ee32ed81ca51e07169844bc6a
    1:11424df6dc1dd4ea255eae2b58eaca7831973bbc
    2:11407b3f1b9c3e76a79c1ec5373924df096f0499
    3:11dd92fe0f39dfdaacdaa5f3997edc533875cfc4
    4:10776689e627b465361ad5c296a20a487e153ca4
    5:a00be79088084cb3aff086ab799f8790e01a976b
    6:a0b0acd79b4498d0052993d35a6a748dd51d13e6
    7:a0457b3450b8e1b778f1163b31a435802987fe5d
    8:c56256a09cd28e5764f32e8e2810d0f01e2e357a
    9:c5623987d205cd6d9d8389bfc40fff9dbb670b48
    10:c562ddd9c94164376c20b86b0b4991636a3bf84f"""
sh % "hg debugobsolete a00be79088084cb3aff086ab799f8790e01a976b" == "obsoleted 1 changesets"
sh % "hg debugobsolete c5623987d205cd6d9d8389bfc40fff9dbb670b48" == "obsoleted 1 changesets"
sh % "hg debugobsolete c562ddd9c94164376c20b86b0b4991636a3bf84f" == "obsoleted 1 changesets"

#  nodes starting with '11' (we don't have the revision number '11' though)

sh % "hg log -r '1:3' -T '{rev}:{shortest(node, 0)}\\n'" == r"""
    1:1142
    2:1140
    3:11d"""

#  '5:a00' is hidden, but still we have two nodes starting with 'a0'

sh % "hg log -r '6:7' -T '{rev}:{shortest(node, 0)}\\n'" == r"""
    6:a0b
    7:a04"""

#  node '10' conflicts with the revision number '10' even if it is hidden
#  (we could exclude hidden revision numbers, but currently we don't)

sh % "hg log -r 4 -T '{rev}:{shortest(node, 0)}\\n'" == "4:107"
sh % "hg log -r 4 -T '{rev}:{shortest(node, 0)}\\n' --hidden" == "4:107"

#  node 'c562' should be unique if the other 'c562' nodes are hidden
#  (but we don't try the slow path to filter out hidden nodes for now)

sh % "hg log -r 8 -T '{rev}:{node|shortest}\\n'" == "8:c5625"
sh % "hg log -r '8:10' -T '{rev}:{node|shortest}\\n' --hidden" == r"""
    8:c5625
    9:c5623
    10:c562d"""

sh % "cd .."

# Test pad function

sh % "cd r"

sh % "hg log --template '{pad(rev, 20)} {author|user}\\n'" == r"""
    2                    test
    1                    {node|short}
    0                    test"""

sh % "hg log --template '{pad(rev, 20, \" \", True)} {author|user}\\n'" == r"""
                       2 test
                       1 {node|short}
                       0 test"""

sh % "hg log --template '{pad(rev, 20, \"-\", False)} {author|user}\\n'" == r"""
    2------------------- test
    1------------------- {node|short}
    0------------------- test"""

# Test unicode fillchar

sh % "'HGENCODING=utf-8' hg log -r 0 -T '{pad(\"hello\", 10, \"\xe2\x98\x83\")}world\\n'" == "hello\xe2\x98\x83\xe2\x98\x83\xe2\x98\x83\xe2\x98\x83\xe2\x98\x83world"

# Test template string in pad function

sh % "hg log -r 0 -T '{pad(\"\\{{rev}}\", 10)} {author|user}\\n'" == "{0}        test"

sh % "hg log -r 0 -T '{pad(r\"\\{rev}\", 10)} {author|user}\\n'" == "\\{rev}     test"

# Test width argument passed to pad function

sh % 'hg log -r 0 -T \'{pad(rev, "1{"0"}")} {author|user}\\n\'' == "0          test"
sh % "hg log -r 0 -T '{pad(rev, \"not an int\")}\\n'" == r"""
    hg: parse error: pad() expects an integer width
    [255]"""

# Test invalid fillchar passed to pad function

sh % "hg log -r 0 -T '{pad(rev, 10, \"\")}\\n'" == r"""
    hg: parse error: pad() expects a single fill character
    [255]"""
sh % "hg log -r 0 -T '{pad(rev, 10, \"--\")}\\n'" == r"""
    hg: parse error: pad() expects a single fill character
    [255]"""

# Test boolean argument passed to pad function

#  no crash

sh % 'hg log -r 0 -T \'{pad(rev, 10, "-", "f{"oo"}")}\\n\'' == "---------0"

#  string/literal

sh % 'hg log -r 0 -T \'{pad(rev, 10, "-", "false")}\\n\'' == "---------0"
sh % "hg log -r 0 -T '{pad(rev, 10, \"-\", false)}\\n'" == "0---------"
sh % 'hg log -r 0 -T \'{pad(rev, 10, "-", "")}\\n\'' == "0---------"

#  unknown keyword is evaluated to ''

sh % "hg log -r 0 -T '{pad(rev, 10, \"-\", unknownkeyword)}\\n'" == "0---------"

# Test separate function

sh % 'hg log -r 0 -T \'{separate("-", "", "a", "b", "", "", "c", "")}\\n\'' == "a-b-c"
sh % 'hg log -r 0 -T \'{separate(" ", "{rev}:{node|short}", author|user, bookmarks)}\\n\'' == "0:f7769ec2ab97 test"
sh % 'hg log -r 0 \'--color=always\' -T \'{separate(" ", "a", label(red, "b"), "c", label(red, ""), "d")}\\n\'' == "a \\x1b[0;31mb\\x1b[0m c d (esc)"

# Test boolean expression/literal passed to if function

sh % "hg log -r 0 -T '{if(rev, \"rev 0 is True\")}\\n'" == "rev 0 is True"
sh % "hg log -r 0 -T '{if(0, \"literal 0 is True as well\")}\\n'" == "literal 0 is True as well"
sh % 'hg log -r 0 -T \'{if("", "", "empty string is False")}\\n\'' == "empty string is False"
sh % 'hg log -r 0 -T \'{if(revset(r"0 - 0"), "", "empty list is False")}\\n\'' == "empty list is False"
sh % "hg log -r 0 -T '{if(true, \"true is True\")}\\n'" == "true is True"
sh % 'hg log -r 0 -T \'{if(false, "", "false is False")}\\n\'' == "false is False"
sh % 'hg log -r 0 -T \'{if("false", "non-empty string is True")}\\n\'' == "non-empty string is True"

# Test ifcontains function

sh % 'hg log --template \'{rev} {ifcontains(rev, "2 two 0", "is in the string", "is not")}\\n\'' == r"""
    2 is in the string
    1 is not
    0 is in the string"""

sh % 'hg log -T \'{rev} {ifcontains(rev, "2 two{" 0"}", "is in the string", "is not")}\\n\'' == r"""
    2 is in the string
    1 is not
    0 is in the string"""

sh % 'hg log --template \'{rev} {ifcontains("a", file_adds, "added a", "did not add a")}\\n\'' == r"""
    2 did not add a
    1 did not add a
    0 added a"""

sh % "hg log --debug -T '{rev}{ifcontains(1, parents, \" is parent of 1\")}\\n'" == r"""
    2 is parent of 1
    1
    0"""

# Test revset function

sh % 'hg log --template \'{rev} {ifcontains(rev, revset("."), "current rev", "not current rev")}\\n\'' == r"""
    2 current rev
    1 not current rev
    0 not current rev"""

sh % 'hg log --template \'{rev} {ifcontains(rev, revset(". + .^"), "match rev", "not match rev")}\\n\'' == r"""
    2 match rev
    1 match rev
    0 not match rev"""

sh % 'hg log -T \'{ifcontains(desc, revset(":"), "", "type not match")}\\n\' -l1' == "type not match"

sh % "hg log --template '{rev} Parents: {revset(\"parents(%s)\", rev)}\\n'" == r"""
    2 Parents: 1
    1 Parents: 0
    0 Parents:"""

sh % "cat" << r"""
[revsetalias]
myparents(\$1) = parents(\$1)
""" >> ".hg/hgrc"
sh % "hg log --template '{rev} Parents: {revset(\"myparents(%s)\", rev)}\\n'" == r"""
    2 Parents: 1
    1 Parents: 0
    0 Parents:"""

sh % 'hg log --template \'Rev: {rev}\\n{revset("::%s", rev) % "Ancestor: {revision}\\n"}\\n\'' == r"""
    Rev: 2
    Ancestor: 0
    Ancestor: 1
    Ancestor: 2

    Rev: 1
    Ancestor: 0
    Ancestor: 1

    Rev: 0
    Ancestor: 0"""
sh % "hg log --template '{revset(\"TIP\"|lower)}\\n' -l1" == "2"

sh % 'hg log -T \'{revset("%s", "t{"ip"}")}\\n\' -l1' == "2"

#  a list template is evaluated for each item of revset/parents

sh % 'hg log -T \'{rev} p: {revset("p1(%s)", rev) % "{rev}:{node|short}"}\\n\'' == r"""
    2 p: 1:ea4c0948489d
    1 p: 0:f7769ec2ab97
    0 p:"""

sh % "hg log --debug -T '{rev} p:{parents % \" {rev}:{node|short}\"}\\n'" == r"""
    2 p: 1:ea4c0948489d -1:000000000000
    1 p: 0:f7769ec2ab97 -1:000000000000
    0 p: -1:000000000000 -1:000000000000"""

#  therefore, 'revcache' should be recreated for each rev

sh % 'hg log -T \'{rev} {file_adds}\\np {revset("p1(%s)", rev) % "{file_adds}"}\\n\'' == r"""
    2 aa b
    p  (trailing space)
    1  (trailing space)
    p a
    0 a
    p"""

sh % "hg log --debug -T '{rev} {file_adds}\\np {parents % \"{file_adds}\"}\\n'" == r"""
    2 aa b
    p  (trailing space)
    1  (trailing space)
    p a
    0 a
    p"""

# a revset item must be evaluated as an integer revision, not an offset from tip

sh % 'hg log -l 1 -T \'{revset("null") % "{rev}:{node|short}"}\\n\'' == "-1:000000000000"
sh % 'hg log -l 1 -T \'{revset("%s", "null") % "{rev}:{node|short}"}\\n\'' == "-1:000000000000"

# join() should pick '{rev}' from revset items:

sh % 'hg log -R ../a -T \'{join(revset("parents(%d)", rev), ", ")}\\n\' -r6' == "4, 5"

# on the other hand, parents are formatted as '{rev}:{node|formatnode}' by
# default. join() should agree with the default formatting:

sh % "hg log -R ../a -T '{join(parents, \", \")}\\n' -r6" == "5:13207e5a10d9, 4:07fa1db10648"

sh % "hg log -R ../a -T '{join(parents, \",\\n\")}\\n' -r6 --debug" == r"""
    5:13207e5a10d9fd28ec424934298e176197f2c67f,
    4:07fa1db1064879a32157227401eb44b322ae53ce"""

# Test files function

sh % "hg log -T '{rev}\\n{join(files('\\''*'\\''), '\\''\\n'\\'')}\\n'" == r"""
    2
    a
    aa
    b
    1
    a
    0
    a"""

sh % "hg log -T '{rev}\\n{join(files('\\''aa'\\''), '\\''\\n'\\'')}\\n'" == r"""
    2
    aa
    1

    0"""

# Test relpath function

sh % "hg log -r0 -T '{files % \"{file|relpath}\\n\"}'" == "a"
sh % "cd .."
sh % "hg log -R r -r0 -T '{files % \"{file|relpath}\\n\"}'" == "r/a"
sh % "cd r"

# Test active bookmark templating

sh % "hg book foo"
sh % "hg book bar"
sh % "hg log --template '{rev} {bookmarks % '\\''{bookmark}{ifeq(bookmark, active, \"*\")} '\\''}\\n'" == r"""
    2 bar* foo text.{rev}  (trailing space)
    1  (trailing space)
    0"""
sh % "hg log --template '{rev} {activebookmark}\\n'" == r"""
    2 bar
    1  (trailing space)
    0"""
sh % "hg bookmarks --inactive bar"
sh % "hg log --template '{rev} {activebookmark}\\n'" == r"""
    2  (trailing space)
    1  (trailing space)
    0"""
sh % "hg book -r1 baz"
sh % "hg log --template '{rev} {join(bookmarks, '\\'' '\\'')}\\n'" == r"""
    2 bar foo text.{rev}
    1 baz
    0"""
sh % "hg log --template '{rev} {ifcontains('\\''foo'\\'', bookmarks, '\\''t'\\'', '\\''f'\\'')}\\n'" == r"""
    2 t
    1 f
    0 f"""

# Test namespaces dict

sh % "hg --config 'extensions.revnamesext=$TESTDIR/revnamesext.py' log -T '{rev}\\n{namespaces % \" {namespace} color={colorname} builtin={builtin}\\n  {join(names, \",\")}\\n\"}\\n'" == r"""
    2
     bookmarks color=bookmark builtin=True
      bar,foo,text.{rev}
     tags color=tag builtin=True
      tip
     branches color=branch builtin=True
      default
     revnames color=revname builtin=False
      r2

    1
     bookmarks color=bookmark builtin=True
      baz
     tags color=tag builtin=True
       (trailing space)
     branches color=branch builtin=True
      default
     revnames color=revname builtin=False
      r1

    0
     bookmarks color=bookmark builtin=True
       (trailing space)
     tags color=tag builtin=True
       (trailing space)
     branches color=branch builtin=True
      default
     revnames color=revname builtin=False
      r0"""
sh % "hg log -r2 -T '{namespaces % \"{namespace}: {names}\\n\"}'" == r"""
    bookmarks: bar foo text.{rev}
    tags: tip
    branches: default"""
sh % 'hg log -r2 -T \'{namespaces % "{namespace}:\\n{names % " {name}\\n"}"}\'' == r"""
    bookmarks:
     bar
     foo
     text.{rev}
    tags:
     tip
    branches:
     default"""
sh % 'hg log -r2 -T \'{get(namespaces, "bookmarks") % "{name}\\n"}\'' == r"""
    bar
    foo
    text.{rev}"""
sh % "hg log -r2 -T '{namespaces.bookmarks % \"{bookmark}\\n\"}'" == r"""
    bar
    foo
    text.{rev}"""

# Test stringify on sub expressions

sh % "cd .."
sh % 'hg log -R a -r 8 --template \'{join(files, if("1", if("1", ", ")))}\\n\'' == "fourth, second, third"
sh % 'hg log -R a -r 8 --template \'{strip(if("1", if("1", "-abc-")), if("1", if("1", "-")))}\\n\'' == "abc"

# Test splitlines

sh % "hg log -Gv -R a --template '{splitlines(desc) % '\\''foo {line}\\n'\\''}'" == r"""
    @  foo Modify, add, remove, rename
    |
    o  foo future
    |
    o  foo third
    |
    o  foo second

    o    foo merge
    |\
    | o  foo new head
    | |
    o |  foo new branch
    |/
    o  foo no user, no domain
    |
    o  foo no person
    |
    o  foo other 1
    |  foo other 2
    |  foo
    |  foo other 3
    o  foo line 1
       foo line 2"""

sh % "hg log -R a -r0 -T '{desc|splitlines}\\n'" == "line 1 line 2"
sh % "hg log -R a -r0 -T '{join(desc|splitlines, \"|\")}\\n'" == "line 1|line 2"

# Test startswith
sh % "hg log -Gv -R a --template '{startswith(desc)}'" == r"""
    hg: parse error: startswith expects two arguments
    [255]"""

sh % "hg log -Gv -R a --template '{startswith('\\''line'\\'', desc)}'" == r"""
    @
    |
    o
    |
    o
    |
    o

    o
    |\
    | o
    | |
    o |
    |/
    o
    |
    o
    |
    o
    |
    o  line 1
       line 2"""

# Test bad template with better error message

sh % "hg log -Gv -R a --template '{desc|user()}'" == r"""
    hg: parse error: expected a symbol, got 'func'
    [255]"""

# Test word function (including index out of bounds graceful failure)

sh % "hg log -Gv -R a --template '{word('\\''1'\\'', desc)}'" == r"""
    @  add,
    |
    o
    |
    o
    |
    o

    o
    |\
    | o  head
    | |
    o |  branch
    |/
    o  user,
    |
    o  person
    |
    o  1
    |
    o  1"""

# Test word third parameter used as splitter

sh % "hg log -Gv -R a --template '{word('\\''0'\\'', desc, '\\''o'\\'')}'" == r"""
    @  M
    |
    o  future
    |
    o  third
    |
    o  sec

    o    merge
    |\
    | o  new head
    | |
    o |  new branch
    |/
    o  n
    |
    o  n
    |
    o
    |
    o  line 1
       line 2"""

# Test word error messages for not enough and too many arguments

sh % "hg log -Gv -R a --template '{word('\\''0'\\'')}'" == r"""
    hg: parse error: word expects two or three arguments, got 1
    [255]"""

sh % "hg log -Gv -R a --template '{word('\\''0'\\'', desc, '\\''o'\\'', '\\''h'\\'', '\\''b'\\'', '\\''o'\\'', '\\''y'\\'')}'" == r"""
    hg: parse error: word expects two or three arguments, got 7
    [255]"""

# Test word for integer literal

sh % "hg log -R a --template '{word(2, desc)}\\n' -r0" == "line"

# Test word for invalid numbers

sh % "hg log -Gv -R a --template '{word('\\''a'\\'', desc)}'" == r"""
    hg: parse error: word expects an integer index
    [255]"""

# Test word for out of range

sh % "hg log -R a --template '{word(10000, desc)}'"
sh % "hg log -R a --template '{word(-10000, desc)}'"

# Test indent and not adding to empty lines

sh % "hg log -T '-----\\n{indent(desc, '\\''.. '\\'', '\\'' . '\\'')}\\n' -r '0:1' -R a" == r"""
    -----
     . line 1
    .. line 2
    -----
     . other 1
    .. other 2

    .. other 3"""

# Test with non-strings like dates

sh % "hg log -T '{indent(date, '\\''   '\\'')}\\n' -r '2:3' -R a" == r"""
       1200000.00
       1300000.00"""

# Test broken string escapes:

sh % "hg log -T 'bogus\\' -R a" == r"""
    hg: parse error: trailing \ in string
    [255]"""
sh % "hg log -T '\\xy' -R a" == r"""
    hg: parse error: invalid \x escape
    [255]"""

# json filter should escape HTML tags so that the output can be embedded in hgweb:

sh % "hg log -T '{'\\''<foo@example.org>'\\''|json}\\n' -R a -l1" == '"\\u003cfoo@example.org\\u003e"'

# Templater supports aliases of symbol and func() styles:

sh % "hg clone -q a aliases"
sh % "cd aliases"
sh % "cat" << r"""
[templatealias]
r = rev
rn = "{r}:{node|short}"
status(c, files) = files % "{c} {file}\n"
utcdate(d) = localdate(d, "UTC")
""" >> ".hg/hgrc"

sh % "hg debugtemplate -vr0 '{rn} {utcdate(date)|isodate}\\n'" == r"""
    (template
      (symbol 'rn')
      (string ' ')
      (|
        (func
          (symbol 'utcdate')
          (symbol 'date'))
        (symbol 'isodate'))
      (string '\n'))
    * expanded:
    (template
      (template
        (symbol 'rev')
        (string ':')
        (|
          (symbol 'node')
          (symbol 'short')))
      (string ' ')
      (|
        (func
          (symbol 'localdate')
          (list
            (symbol 'date')
            (string 'UTC')))
        (symbol 'isodate'))
      (string '\n'))
    0:1e4e1b8f71e0 1970-01-12 13:46 +0000"""

sh % "hg debugtemplate -vr0 '{status(\"A\", file_adds)}'" == r"""
    (template
      (func
        (symbol 'status')
        (list
          (string 'A')
          (symbol 'file_adds'))))
    * expanded:
    (template
      (%
        (symbol 'file_adds')
        (template
          (string 'A')
          (string ' ')
          (symbol 'file')
          (string '\n'))))
    A a"""

# A unary function alias can be called as a filter:

sh % "hg debugtemplate -vr0 '{date|utcdate|isodate}\\n'" == r"""
    (template
      (|
        (|
          (symbol 'date')
          (symbol 'utcdate'))
        (symbol 'isodate'))
      (string '\n'))
    * expanded:
    (template
      (|
        (func
          (symbol 'localdate')
          (list
            (symbol 'date')
            (string 'UTC')))
        (symbol 'isodate'))
      (string '\n'))
    1970-01-12 13:46 +0000"""

# Aliases should be applied only to command arguments and templates in hgrc.
# Otherwise, our stock styles and web templates could be corrupted:

sh % "hg log -r0 -T '{rn} {utcdate(date)|isodate}\\n'" == "0:1e4e1b8f71e0 1970-01-12 13:46 +0000"

sh % "hg log -r0 --config 'ui.logtemplate=\"{rn} {utcdate(date)|isodate}\\n\"'" == "0:1e4e1b8f71e0 1970-01-12 13:46 +0000"

sh % "cat" << r"""
changeset = 'nothing expanded:{rn}\n'
""" > "tmpl"
sh % "hg log -r0 --style ./tmpl" == "nothing expanded:"

# Aliases in formatter:

sh % "hg bookmarks -T '{pad(bookmark, 7)} {rn}\\n'" == "foo     4:07fa1db10648"

# Aliases should honor HGPLAIN:

sh % "'HGPLAIN=' hg log -r0 -T 'nothing expanded:{rn}\\n'" == "nothing expanded:"
sh % "'HGPLAINEXCEPT=templatealias' hg log -r0 -T '{rn}\\n'" == "0:1e4e1b8f71e0"

# Unparsable alias:

sh % "hg debugtemplate --config 'templatealias.bad=x(' -v '{bad}'" == r"""
    (template
      (symbol 'bad'))
    abort: bad definition of template alias "bad": at 2: not a prefix: end
    [255]"""
sh % "hg log --config 'templatealias.bad=x(' -T '{bad}'" == r"""
    abort: bad definition of template alias "bad": at 2: not a prefix: end
    [255]"""

sh % "cd .."

# Set up repository for non-ascii encoding tests:

sh % "hg init nonascii"
sh % "cd nonascii"
sh % "'$PYTHON'" << r"""
open('latin1', 'w').write('\xe9')
open('utf-8', 'w').write('\xc3\xa9')
"""
sh % "'HGENCODING=utf-8' hg bookmark -q '`cat' 'utf-8`'"
sh % "'HGENCODING=utf-8' hg ci -qAm 'non-ascii branch: `cat utf-8`' utf-8"

# json filter should try round-trip conversion to utf-8:

sh % "'HGENCODING=ascii' hg log -T '{bookmarks|json}\\n' -r0" == '["\\u00e9"]'
sh % "'HGENCODING=ascii' hg log -T '{desc|json}\\n' -r0" == '"non-ascii branch: \\u00e9"'

# json filter takes input as utf-8b:

sh % "'HGENCODING=ascii' hg log -T '{'\\''`cat utf-8`'\\''|json}\\n' -l1" == '"\\u00e9"'
sh % "'HGENCODING=ascii' hg log -T '{'\\''`cat latin1`'\\''|json}\\n' -l1" == r"""
    abort: cannot decode command line arguments
    [255]"""

# utf8 filter:

sh % "'HGENCODING=ascii' hg log -T 'round-trip: {bookmarks % '\\''{bookmark|utf8|hex}'\\''}\\n' -r0" == "round-trip: c3a9"
sh % "'HGENCODING=latin1' hg log -T 'decoded: {'\\''`cat latin1`'\\''|utf8|hex}\\n' -l1" == r"""
    abort: cannot decode command line arguments
    [255]"""
sh % "'HGENCODING=ascii' hg log -T 'replaced: {'\\''`cat latin1`'\\''|utf8|hex}\\n' -l1" == r"""
    abort: cannot decode command line arguments
    [255]"""
sh % "hg log -T 'invalid type: {rev|utf8}\\n' -r0" == r"""
    abort: template filter 'utf8' is not compatible with keyword 'rev'
    [255]"""

# pad width:

sh % "'HGENCODING=utf-8' hg debugtemplate '{pad('\\''`cat utf-8`'\\'', 2, '\\''-'\\'')}\\n'" == "\\xc3\\xa9- (esc)"

sh % "cd .."

# Test that template function in extension is registered as expected

sh % "cd a"

sh % "cat" << r"""
from edenscm.mercurial import registrar

templatefunc = registrar.templatefunc()

@templatefunc('custom()')
def custom(context, mapping, args):
    return 'custom'
""" > "$TESTTMP/customfunc.py"
sh % "cat" << r"""
[extensions]
customfunc = $TESTTMP/customfunc.py
""" > ".hg/hgrc"

sh % "hg log -r . -T '{custom()}\\n' --config 'customfunc.enabled=true'" == "custom"

sh % "cd .."

# Test 'graphwidth' in 'hg log' on various topologies. The key here is that the
# printed graphwidths 3, 5, 7, etc. should all line up in their respective
# columns. We don't care about other aspects of the graph rendering here.

sh % "hg init graphwidth"
sh % "cd graphwidth"

sh % "'wrappabletext=a a a a a a a a a a a a'"

sh % "printf 'first\\n'" > "file"
sh % "hg add file"
sh % "hg commit -m '$wrappabletext'"

sh % "printf 'first\\nsecond\\n'" > "file"
sh % "hg commit -m '$wrappabletext'"

sh % "hg checkout 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "printf 'third\\nfirst\\n'" > "file"
sh % "hg commit -m '$wrappabletext'"

sh % "hg merge" == r"""
    merging file
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

sh % "hg log --graph -T '{graphwidth}'" == r"""
    @  3
    |
    | @  5
    |/
    o  3"""
sh % "hg commit -m '$wrappabletext'"

sh % "hg log --graph -T '{graphwidth}'" == r"""
    @    5
    |\
    | o  5
    | |
    o |  5
    |/
    o  3"""

sh % "hg checkout 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "printf 'third\\nfirst\\nsecond\\n'" > "file"
sh % "hg commit -m '$wrappabletext'"

sh % "hg log --graph -T '{graphwidth}'" == r"""
    @  3
    |
    | o    7
    | |\
    +---o  7
    | |
    | o  5
    |/
    o  3"""

sh % "hg log --graph -T '{graphwidth}' -r 3" == r"""
    o    5
    |\
    ~ ~"""

sh % "hg log --graph -T '{graphwidth}' -r 1" == r"""
    o  3
    |
    ~"""

sh % "hg merge" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg commit -m '$wrappabletext'"

sh % "printf 'seventh\\n'" >> "file"
sh % "hg commit -m '$wrappabletext'"

sh % "hg log --graph -T '{graphwidth}'" == r"""
    @  3
    |
    o    5
    |\
    | o  5
    | |
    o |    7
    |\ \
    | o |  7
    | |/
    o /  5
    |/
    o  3"""

# The point of graphwidth is to allow wrapping that accounts for the space taken
# by the graph.

sh % "'COLUMNS=10' hg log --graph -T '{fill(desc, termwidth - graphwidth)}'" == r"""
    @  a a a a
    |  a a a a
    |  a a a a
    o    a a a
    |\   a a a
    | |  a a a
    | |  a a a
    | o  a a a
    | |  a a a
    | |  a a a
    | |  a a a
    o |    a a
    |\ \   a a
    | | |  a a
    | | |  a a
    | | |  a a
    | | |  a a
    | o |  a a
    | |/   a a
    | |    a a
    | |    a a
    | |    a a
    | |    a a
    o |  a a a
    |/   a a a
    |    a a a
    |    a a a
    o  a a a a
       a a a a
       a a a a"""

# Something tricky happens when there are elided nodes; the next drawn row of
# edges can be more than one column wider, but the graph width only increases by
# one column. The remaining columns are added in between the nodes.

sh % "hg log --graph -T '{graphwidth}' -r '0|2|4|5'" == r"""
    o    5
    |\
    | \
    | :\
    o : :  7
    :/ /
    : o  5
    :/
    o  3"""

sh % "cd .."

# Confirm that truncation does the right thing

sh % "hg debugtemplate '{truncatelonglines(\"abcdefghijklmnopqrst\\n\", 10)}'" == "abcdefghij"
sh % 'hg debugtemplate \'{truncatelonglines("abcdefghijklmnopqrst\\n", 10, "\xe2\x80\xa6")}\'' == "abcdefghi\\xe2\\x80\\xa6 (esc)"
sh % "hg debugtemplate '{truncate(\"a\\nb\\nc\\n\", 2)}'" == r"""
    a
    b"""
sh % 'hg debugtemplate \'{truncate("a\\nb\\nc\\n", 2, "truncated\\n")}\'' == r"""
    a
    truncated"""
