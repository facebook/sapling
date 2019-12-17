# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

# Log on empty repository: checking consistency

sh % "hg init empty"
sh % "cd empty"
sh % "hg log"
sh % "hg log -r 1" == r"""
    abort: unknown revision '1'!
    (if 1 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "hg log -r '-1:0'" == r"""
    abort: unknown revision '-1'!
    (if -1 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""
sh % "hg log -r 'branch(name)'"
sh % "hg log -r null -q" == "-1:000000000000"

sh % "cd .."

# The g is crafted to have 2 filelog topological heads in a linear
# changeset graph

sh % "hg init a"
sh % "cd a"
sh % "echo a" > "a"
sh % "echo f" > "f"
sh % "hg ci -Ama -d '1 0'" == r"""
    adding a
    adding f"""

sh % "hg cp a b"
sh % "hg cp f g"
sh % "hg ci -mb -d '2 0'"

sh % "mkdir dir"
sh % "hg mv b dir"
sh % "echo g" >> "g"
sh % "echo f" >> "f"
sh % "hg ci -mc -d '3 0'"

sh % "hg mv a b"
sh % "hg cp -f f g"
sh % "echo a" > "d"
sh % "hg add d"
sh % "hg ci -md -d '4 0'"

sh % "hg mv dir/b e"
sh % "hg ci -me -d '5 0'"

sh % "hg --debug log a -T '{rev}: {desc}\\n'" == "0: a"
sh % "hg log a" == r"""
    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a"""
sh % "hg log 'glob:a*'" == r"""
    changeset:   3:2ca5ba701980
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     d

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a"""
sh % "hg --debug log 'glob:a*' -T '{rev}: {desc}\\n'" == r"""
    3: d
    0: a"""

# log on directory

sh % "hg log dir" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     e

    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c"""
sh % "hg log somethingthatdoesntexist dir" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     e

    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c"""

# -f, non-existent directory

sh % "hg log -f dir" == r"""
    abort: cannot follow file not in parent revision: "dir"
    [255]"""

# -f, directory
# (The code path using "follow()" revset will follow file renames, so 'b' and 'a' show up)

sh % "hg up -q 3"
sh % "hg log -f dir" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a"""
# -f, directory with --patch

sh % "hg log -f dir -p" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
    --- /dev/null* (glob)
    +++ b/dir/b* (glob)
    @@ -0,0 +1,1 @@
    +a

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b


    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a"""

# -f, pattern

sh % "hg log -f -I 'dir**' -p" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
    --- /dev/null* (glob)
    +++ b/dir/b* (glob)
    @@ -0,0 +1,1 @@
    +a"""
sh % "hg up -q 4"

# -f, a wrong style

sh % "hg log -f -l1 --style something" == r"""
    abort: style 'something' not found
    (available styles: bisect, changelog, compact, default, phases, show, status, xml)
    [255]"""

# -f, phases style


sh % "hg log -f -l1 --style phases" == r"""
    changeset:   4:7e4639b4691b
    phase:       draft
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     e"""

sh % "hg log -f -l1 --style phases -q" == "4:7e4639b4691b"

# -f, but no args

sh % "hg log -f" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     e

    changeset:   3:2ca5ba701980
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     d

    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a"""

# one rename

sh % "hg up -q 2"
sh % "hg log -vf a" == r"""
    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    files:       a f
    description:
    a"""

# many renames

sh % "hg up -q tip"
sh % "hg log -vf e" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    files:       dir/b e
    description:
    e


    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    files:       b dir/b f g
    description:
    c


    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    files:       b g
    description:
    b


    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    files:       a f
    description:
    a"""


# log -pf dir/b

sh % "hg up -q 3"
sh % "hg log -pf dir/b" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
    @@ -0,0 +1,1 @@
    +a

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b

    diff -r 9161b9aeaf16 -r d89b0a12d229 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:02 1970 +0000
    @@ -0,0 +1,1 @@
    +a

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a

    diff -r 000000000000 -r 9161b9aeaf16 a
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:01 1970 +0000
    @@ -0,0 +1,1 @@
    +a"""

# log -pf b inside dir

sh % "hg '--cwd=dir' log -pf b" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
    @@ -0,0 +1,1 @@
    +a

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b

    diff -r 9161b9aeaf16 -r d89b0a12d229 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:02 1970 +0000
    @@ -0,0 +1,1 @@
    +a

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a

    diff -r 000000000000 -r 9161b9aeaf16 a
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:01 1970 +0000
    @@ -0,0 +1,1 @@
    +a"""

# log -pf, but no args

sh % "hg log -pf" == r"""
    changeset:   3:2ca5ba701980
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     d

    diff -r f8954cd4dc1f -r 2ca5ba701980 a
    --- a/a	Thu Jan 01 00:00:03 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r f8954cd4dc1f -r 2ca5ba701980 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:04 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r f8954cd4dc1f -r 2ca5ba701980 d
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/d	Thu Jan 01 00:00:04 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r f8954cd4dc1f -r 2ca5ba701980 g
    --- a/g	Thu Jan 01 00:00:03 1970 +0000
    +++ b/g	Thu Jan 01 00:00:04 1970 +0000
    @@ -1,2 +1,2 @@
     f
    -g
    +f

    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     c

    diff -r d89b0a12d229 -r f8954cd4dc1f b
    --- a/b	Thu Jan 01 00:00:02 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r d89b0a12d229 -r f8954cd4dc1f f
    --- a/f	Thu Jan 01 00:00:02 1970 +0000
    +++ b/f	Thu Jan 01 00:00:03 1970 +0000
    @@ -1,1 +1,2 @@
     f
    +f
    diff -r d89b0a12d229 -r f8954cd4dc1f g
    --- a/g	Thu Jan 01 00:00:02 1970 +0000
    +++ b/g	Thu Jan 01 00:00:03 1970 +0000
    @@ -1,1 +1,2 @@
     f
    +g

    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     b

    diff -r 9161b9aeaf16 -r d89b0a12d229 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:02 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r 9161b9aeaf16 -r d89b0a12d229 g
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/g	Thu Jan 01 00:00:02 1970 +0000
    @@ -0,0 +1,1 @@
    +f

    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     a

    diff -r 000000000000 -r 9161b9aeaf16 a
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:01 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r 000000000000 -r 9161b9aeaf16 f
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/f	Thu Jan 01 00:00:01 1970 +0000
    @@ -0,0 +1,1 @@
    +f"""

# log -vf dir/b

sh % "hg log -vf dir/b" == r"""
    changeset:   2:f8954cd4dc1f
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    files:       b dir/b f g
    description:
    c


    changeset:   1:d89b0a12d229
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    files:       b g
    description:
    b


    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    files:       a f
    description:
    a"""


# -f and multiple filelog heads

sh % "hg up -q 2"
sh % "hg log -f g --template '{rev}\\n'" == r"""
    2
    1
    0"""
sh % "hg up -q tip"
sh % "hg log -f g --template '{rev}\\n'" == r"""
    3
    2
    0"""


# log copies with --copies

sh % "hg log -vC --template '{rev} {file_copies}\\n'" == r"""
    4 e (dir/b)
    3 b (a)g (f)
    2 dir/b (b)
    1 b (a)g (f)
    0"""

# log copies switch without --copies, with old filecopy template

sh % "hg log -v --template '{rev} {file_copies_switch%filecopy}\\n'" == r"""
    4  (trailing space)
    3  (trailing space)
    2  (trailing space)
    1  (trailing space)
    0"""

# log copies switch with --copies

sh % "hg log -vC --template '{rev} {file_copies_switch}\\n'" == r"""
    4 e (dir/b)
    3 b (a)g (f)
    2 dir/b (b)
    1 b (a)g (f)
    0"""


# log copies with hardcoded style and with --style=default

sh % "hg log -vC -r4" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    files:       dir/b e
    copies:      e (dir/b)
    description:
    e"""
sh % "hg log -vC -r4 '--style=default'" == r"""
    changeset:   4:7e4639b4691b
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    files:       dir/b e
    copies:      e (dir/b)
    description:
    e"""
sh % "hg log -vC -r4 -Tjson" == r"""
    [
     {
      "rev": 4,
      "node": "7e4639b4691b9f84b81036a8d4fb218ce3c5e3a3",
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [5, 0],
      "desc": "e",
      "bookmarks": [],
      "tags": [],
      "parents": ["2ca5ba7019804f1f597249caddf22a64d34df0ba"],
      "files": ["dir/b", "e"],
      "copies": {"e": "dir/b"}
     }
    ]"""

# log copies, non-linear manifest

sh % "hg up -C 3" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg mv dir/b e"
sh % "echo foo" > "foo"
sh % "hg ci -Ame2 -d '6 0'" == "adding foo"
sh % "hg log -v --template '{rev} {file_copies}\\n' -r 5" == "5 e (dir/b)"


# log copies, execute bit set

if feature.check(["execbit"]):
    sh % "chmod +x e"
    sh % "hg ci -me3 -d '7 0'"
    sh % "hg log -v --template '{rev} {file_copies}\\n' -r 6" == "6"


# log -p d

sh % "hg log -pv d" == r"""
    changeset:   3:2ca5ba701980
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    files:       a b d g
    description:
    d


    diff -r f8954cd4dc1f -r 2ca5ba701980 d
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/d	Thu Jan 01 00:00:04 1970 +0000
    @@ -0,0 +1,1 @@
    +a"""


# log --removed file

sh % "hg log --removed -v a" == r"""
    changeset:   3:2ca5ba701980
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    files:       a b d g
    description:
    d


    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    files:       a f
    description:
    a"""

# log --removed revrange file

sh % "hg log --removed -v '-r0:2' a" == r"""
    changeset:   0:9161b9aeaf16
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    files:       a f
    description:
    a"""
sh % "cd .."

# log --follow tests

sh % "hg init follow"
sh % "cd follow"

sh % "echo base" > "base"
sh % "hg ci -Ambase -d '1 0'" == "adding base"

sh % "echo r1" >> "base"
sh % "hg ci -Amr1 -d '1 0'"
sh % "echo r2" >> "base"
sh % "hg ci -Amr2 -d '1 0'"

sh % "hg up -C 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo b1" > "b1"

# log -r "follow('set:clean()')"

sh % "hg log -r 'follow('\\''set:clean()'\\'')'" == r"""
    changeset:   0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     base

    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1"""

sh % "hg ci -Amb1 -d '1 0'" == "adding b1"


# log -f

sh % "hg log -f" == r"""
    changeset:   3:e62f78d544b4
    parent:      1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1

    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1

    changeset:   0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     base"""

# log -r follow('glob:b*')

sh % "hg log -r 'follow('\\''glob:b*'\\'')'" == r"""
    changeset:   0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     base

    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1

    changeset:   3:e62f78d544b4
    parent:      1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1"""
# log -f -r '1 + 4'

sh % "hg up -C 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo b2" > "b2"
sh % "hg ci -Amb2 -d '1 0'" == "adding b2"
sh % "hg log -f -r '1 + 4'" == r"""
    changeset:   4:ddb82e70d1a1
    parent:      0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b2

    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1

    changeset:   0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     base"""
# log -r "follow('set:grep(b2)')"

sh % "hg log -r 'follow('\\''set:grep(b2)'\\'')'" == r"""
    changeset:   4:ddb82e70d1a1
    parent:      0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b2"""
# log -r "follow('set:grep(b2)', 4)"

sh % "hg up -qC 0"
sh % "hg log -r 'follow('\\''set:grep(b2)'\\'', 4)'" == r"""
    changeset:   4:ddb82e70d1a1
    parent:      0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b2"""

# follow files starting from multiple revisions:

sh % "hg log -T '{rev}: {files}\\n' -r 'follow('\\''glob:b?'\\'', startrev=2+3+4)'" == r"""
    3: b1
    4: b2"""

# follow files starting from empty revision:

sh % "hg log -T '{rev}: {files}\\n' -r 'follow('\\''glob:*'\\'', startrev=.-.)'"

# follow starting from revisions:

sh % "hg log -Gq -r 'follow(startrev=2+4)'" == r"""
    o  4:ddb82e70d1a1
    |
    | o  2:60c670bf5b30
    | |
    | o  1:3d5bf5654eda
    |/
    @  0:67e992f2c4f3"""

# follow the current revision:

sh % "hg log -Gq -r 'follow()'" == "@  0:67e992f2c4f3"

sh % "hg up -qC 4"

# log -f -r null

sh % "hg log -f -r null" == r"""
    changeset:   -1:000000000000
    user:         (trailing space)
    date:        Thu Jan 01 00:00:00 1970 +0000"""
sh % "hg log -f -r null -G" == r"""
    o  changeset:   -1:000000000000
       user:
       date:        Thu Jan 01 00:00:00 1970 +0000"""


# log -f with null parent

sh % "hg up -C null" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "hg log -f"


# log -r .  with two parents

sh % "hg up -C 3" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg merge tip" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg log -r ." == r"""
    changeset:   3:e62f78d544b4
    parent:      1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1"""


# log -r .  with one parent

sh % "hg ci -mm12 -d '1 0'"
sh % "hg log -r ." == r"""
    changeset:   5:302e9dd6890d
    parent:      3:e62f78d544b4
    parent:      4:ddb82e70d1a1
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     m12"""

sh % "echo postm" >> "b1"
sh % "hg ci -Amb1.1 '-d1 0'"


# log --follow-first

sh % "hg log --follow-first" == r"""
    changeset:   6:2404bbcab562
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1.1

    changeset:   5:302e9dd6890d
    parent:      3:e62f78d544b4
    parent:      4:ddb82e70d1a1
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     m12

    changeset:   3:e62f78d544b4
    parent:      1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1

    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1

    changeset:   0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     base"""


# log -P 2

sh % "hg log -P 2" == r"""
    changeset:   6:2404bbcab562
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1.1

    changeset:   5:302e9dd6890d
    parent:      3:e62f78d544b4
    parent:      4:ddb82e70d1a1
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     m12

    changeset:   4:ddb82e70d1a1
    parent:      0:67e992f2c4f3
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b2

    changeset:   3:e62f78d544b4
    parent:      1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1"""


# log -r tip -p --git

sh % "hg log -r tip -p --git" == r"""
    changeset:   6:2404bbcab562
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1.1

    diff --git a/b1 b/b1
    --- a/b1
    +++ b/b1
    @@ -1,1 +1,2 @@
     b1
    +postm"""


# log -r ""

sh % "hg log -r ''" == r"""
    hg: parse error: empty query
    [255]"""

# log -r <some unknown node id>

sh % "hg log -r 1000000000000000000000000000000000000000" == r"""
    abort: unknown revision '1000000000000000000000000000000000000000'!
    (if 1000000000000000000000000000000000000000 is a remote bookmark or commit, try to 'hg pull' it first)
    [255]"""

# log -k r1

sh % "hg log -k r1" == r"""
    changeset:   1:3d5bf5654eda
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     r1"""
# log -p -l2 --color=always

sh % "hg --config 'extensions.color=' --config 'color.mode=ansi' log -p -l2 '--color=always'" == r"""
    [0;33mchangeset:   6:2404bbcab562[0m
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1.1

    [0;1mdiff -r 302e9dd6890d -r 2404bbcab562 b1[0m
    [0;31;1m--- a/b1 Thu Jan 01 00:00:01 1970 +0000[0m
    [0;32;1m+++ b/b1 Thu Jan 01 00:00:01 1970 +0000[0m
    [0;35m@@ -1,1 +1,2 @@[0m
     b1
    [0;92m+postm[0m

    [0;33mchangeset:   5:302e9dd6890d[0m
    parent:      3:e62f78d544b4
    parent:      4:ddb82e70d1a1
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     m12

    [0;1mdiff -r e62f78d544b4 -r 302e9dd6890d b2[0m
    [0;31;1m--- /dev/null Thu Jan 01 00:00:00 1970 +0000[0m
    [0;32;1m+++ b/b2 Thu Jan 01 00:00:01 1970 +0000[0m
    [0;35m@@ -0,0 +1,1 @@[0m
    [0;92m+b2[0m"""


# log -r tip --stat

sh % "hg log -r tip --stat" == r"""
    changeset:   6:2404bbcab562
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     b1.1

     b1 |  1 +
     1 files changed, 1 insertions(+), 0 deletions(-)"""

sh % "cd .."

# log --follow --patch FILE in repository where linkrev isn't trustworthy
# (issue5376)

sh % "hg init follow-dup"
sh % "cd follow-dup"
sh % "cat" << r"""
[ui]
logtemplate = '=== {rev}: {desc}\n'
[diff]
nodates = True
""" >> ".hg/hgrc"
sh % "echo 0" >> "a"
sh % "hg ci -qAm a0"
sh % "echo 1" >> "a"
sh % "hg ci -m a1"
sh % "hg up -q 0"
sh % "echo 1" >> "a"
sh % "touch b"
sh % "hg ci -qAm 'a1 with b'"
sh % "echo 3" >> "a"
sh % "hg ci -m a3"

#  fctx.rev() == 2, but fctx.linkrev() == 1

sh % "hg log -pf a" == r"""
    === 3: a3
    diff -r 4ea02ba94d66 -r e7a6331a34f0 a
    --- a/a
    +++ b/a
    @@ -1,2 +1,3 @@
     0
     1
    +3

    === 2: a1 with b
    diff -r 49b5e81287e2 -r 4ea02ba94d66 a
    --- a/a
    +++ b/a
    @@ -1,1 +1,2 @@
     0
    +1

    === 0: a0
    diff -r 000000000000 -r 49b5e81287e2 a
    --- /dev/null
    +++ b/a
    @@ -0,0 +1,1 @@
    +0"""

#  fctx.introrev() == 2, but fctx.linkrev() == 1

sh % "hg up -q 2"
sh % "hg log -pf a" == r"""
    === 2: a1 with b
    diff -r 49b5e81287e2 -r 4ea02ba94d66 a
    --- a/a
    +++ b/a
    @@ -1,1 +1,2 @@
     0
    +1

    === 0: a0
    diff -r 000000000000 -r 49b5e81287e2 a
    --- /dev/null
    +++ b/a
    @@ -0,0 +1,1 @@
    +0"""

sh % "cd .."

# Multiple copy sources of a file:

sh % "hg init follow-multi"
sh % "cd follow-multi"
sh % "echo 0" >> "a"
sh % "hg ci -qAm a"
sh % "hg cp a b"
sh % "hg ci -m 'a->b'"
sh % "echo 2" >> "a"
sh % "hg ci -m a"
sh % "echo 3" >> "b"
sh % "hg ci -m b"
sh % "echo 4" >> "a"
sh % "echo 4" >> "b"
sh % "hg ci -m 'a,b'"
sh % "echo 5" >> "a"
sh % "hg ci -m a0"
sh % "echo 6" >> "b"
sh % "hg ci -m b0"
sh % "hg up -q 4"
sh % "echo 7" >> "b"
sh % "hg ci -m b1"
sh % "echo 8" >> "a"
sh % "hg ci -m a1"
sh % "hg rm a"
sh % "hg mv b a"
sh % "hg ci -m 'b1->a1'"
sh % "hg merge -qt ':local'"
sh % "hg ci -m '(a0,b1->a1)->a'"

sh % "hg log -GT '{rev}: {desc}\\n'" == r"""
    @    10: (a0,b1->a1)->a
    |\
    | o  9: b1->a1
    | |
    | o  8: a1
    | |
    | o  7: b1
    | |
    o |  6: b0
    | |
    o |  5: a0
    |/
    o  4: a,b
    |
    o  3: b
    |
    o  2: a
    |
    o  1: a->b
    |
    o  0: a"""

#  since file 'a' has multiple copy sources at the revision 4, ancestors can't
#  be indexed solely by fctx.linkrev().

sh % "hg log -T '{rev}: {desc}\\n' -f a" == r"""
    10: (a0,b1->a1)->a
    9: b1->a1
    7: b1
    5: a0
    4: a,b
    3: b
    2: a
    1: a->b
    0: a"""

sh % "cd .."

# Test that log should respect the order of -rREV even if multiple OR conditions
# are specified (issue5100):

sh % "hg init revorder"
sh % "cd revorder"

sh % "hg book -q b0"
sh % "echo 0" >> "f0"
sh % "hg ci -qAm k0 -u u0"
sh % "hg book -q b1"
sh % "echo 1" >> "f1"
sh % "hg ci -qAm k1 -u u1"
sh % "hg book -q b2"
sh % "echo 2" >> "f2"
sh % "hg ci -qAm k2 -u u2"

sh % "hg update -q b2"
sh % "echo 3" >> "f2"
sh % "hg ci -qAm k2 -u u2"
sh % "hg update -q b1"
sh % "echo 4" >> "f1"
sh % "hg ci -qAm k1 -u u1"
sh % "hg update -q b0"
sh % "echo 5" >> "f0"
sh % "hg ci -qAm k0 -u u0"

#  summary of revisions:

sh % "hg log -G -T '{rev} {bookmarks} {author} {desc} {files}\\n'" == r"""
    @  5 b0 u0 k0 f0
    |
    | o  4 b1 u1 k1 f1
    | |
    | | o  3 b2 u2 k2 f2
    | | |
    | | o  2  u2 k2 f2
    | |/
    | o  1  u1 k1 f1
    |/
    o  0  u0 k0 f0"""

#  log -u USER in ascending order, against compound set:

sh % "hg log '-r::head()' -T '{rev} {author}\\n' -u u0 -u u2" == r"""
    0 u0
    2 u2
    3 u2
    5 u0"""
sh % "hg log '-r::head()' -T '{rev} {author}\\n' -u u2 -u u0" == r"""
    0 u0
    2 u2
    3 u2
    5 u0"""

#  log -k TEXT in descending order, against compound set:

sh % "hg log '-r5 + reverse(::3)' -T '{rev} {desc}\\n' -k k0 -k k1 -k k2" == r"""
    5 k0
    3 k2
    2 k2
    1 k1
    0 k0"""
sh % "hg log '-r5 + reverse(::3)' -T '{rev} {desc}\\n' -k k2 -k k1 -k k0" == r"""
    5 k0
    3 k2
    2 k2
    1 k1
    0 k0"""

#  log FILE in ascending order, against dagrange:

sh % "hg log '-r1::' -T '{rev} {files}\\n' f1 f2" == r"""
    1 f1
    2 f2
    3 f2
    4 f1"""
sh % "hg log '-r1::' -T '{rev} {files}\\n' f2 f1" == r"""
    1 f1
    2 f2
    3 f2
    4 f1"""

sh % "cd .."

# User

sh % "hg init usertest"
sh % "cd usertest"

sh % "echo a" > "a"
sh % "hg ci -A -m a -u 'User One <user1@example.org>'" == "adding a"
sh % "echo b" > "b"
sh % "hg ci -A -m b -u 'User Two <user2@example.org>'" == "adding b"

sh % "hg log -u 'User One <user1@example.org>'" == r"""
    changeset:   0:29a4c94f1924
    user:        User One <user1@example.org>
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a"""
sh % "hg log -u user1 -u user2" == r"""
    changeset:   1:e834b5e69c0e
    user:        User Two <user2@example.org>
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     b

    changeset:   0:29a4c94f1924
    user:        User One <user1@example.org>
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a"""
sh % "hg log -u user3"

sh % "cd .."

sh % "hg init branches"
sh % "cd branches"

sh % "echo a" > "a"
sh % "hg ci -A -m 'commit on default'" == "adding a"
sh % "hg book test"
sh % "echo b" > "b"
sh % "hg ci -A -m 'commit on test'" == "adding b"

sh % "hg up default" == r"""
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (leaving bookmark test)"""
sh % "echo c" > "c"
sh % "hg ci -A -m 'commit on default'" == "adding c"
sh % "hg up test" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    (activating bookmark test)"""
sh % "echo c" > "c"
sh % "hg ci -A -m 'commit on test'" == "adding c"

# This test is skipped - LANGUAGE side effect is not applied in t.py tests.
if feature.check("false"):

    # Test that all log names are translated (e.g. branches, bookmarks, tags):

    sh % "hg bookmark babar -r tip"

    sh % "'HGENCODING=UTF-8' 'LANGUAGE=de' hg log -r tip" == r"""
        \xc3\x84nderung:        3:91f0fa364897 (esc)
        Lesezeichen:     babar
        Lesezeichen:     test
        Marke:           tip
        Vorg\xc3\xa4nger:       1:45efe61fb969 (esc)
        Nutzer:          test
        Datum:           Thu Jan 01 00:00:00 1970 +0000
        Zusammenfassung: commit on test"""
    sh % "hg bookmark -d babar"


# log -p --cwd dir (in subdir)

sh % "mkdir dir"
sh % "hg log -p --cwd dir" == r"""
    changeset:   3:91f0fa364897
    bookmark:    test
    parent:      1:45efe61fb969
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     commit on test

    diff -r 45efe61fb969 -r 91f0fa364897 c
    --- /dev/null Thu Jan 01 00:00:00 1970 +0000
    +++ b/c Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +c

    changeset:   2:735dba46f54d
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     commit on default

    diff -r 45efe61fb969 -r 735dba46f54d c
    --- /dev/null Thu Jan 01 00:00:00 1970 +0000
    +++ b/c Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +c

    changeset:   1:45efe61fb969
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     commit on test

    diff -r 24427303d56f -r 45efe61fb969 b
    --- /dev/null Thu Jan 01 00:00:00 1970 +0000
    +++ b/b Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +b

    changeset:   0:24427303d56f
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     commit on default

    diff -r 000000000000 -r 24427303d56f a
    --- /dev/null Thu Jan 01 00:00:00 1970 +0000
    +++ b/a Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a"""


# log -p -R repo

sh % "cd dir"
sh % "hg log -p -R .. ../a" == r"""
    changeset:   0:24427303d56f
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     commit on default

    diff -r 000000000000 -r 24427303d56f a
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a"""

sh % "cd ../.."

sh % "hg init follow2"
sh % "cd follow2"

# Build the following history:
# tip - o - x - o - x - x
#    \                 /
#     o - o - o - x
#      \     /
#         o
#
# Where "o" is a revision containing "foo" and
# "x" is a revision without "foo"

sh % "touch init"
sh % "hg ci -A -m 'init, unrelated'" == "adding init"
sh % "echo foo" > "init"
sh % "hg ci -m 'change, unrelated'"
sh % "echo foo" > "foo"
sh % "hg ci -A -m 'add unrelated old foo'" == "adding foo"
sh % "hg rm foo"
sh % "hg ci -m 'delete foo, unrelated'"
sh % "echo related" > "foo"
sh % "hg ci -A -m 'add foo, related'" == "adding foo"

sh % "hg up 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "touch branch"
sh % "hg ci -A -m 'first branch, unrelated'" == "adding branch"
sh % "touch foo"
sh % "hg ci -A -m 'create foo, related'" == "adding foo"
sh % "echo change" > "foo"
sh % "hg ci -m 'change foo, related'"

sh % "hg up 6" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo 'change foo in branch'" > "foo"
sh % "hg ci -m 'change foo in branch, related'"
sh % "hg merge 7" == r"""
    merging foo
    warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
    0 files updated, 0 files merged, 0 files removed, 1 files unresolved
    use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
    [1]"""
sh % "echo 'merge 1'" > "foo"
sh % "hg resolve -m foo" == "(no more unresolved files)"
sh % "hg ci -m 'First merge, related'"

sh % "hg merge 4" == r"""
    merging foo
    warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
    1 files updated, 0 files merged, 0 files removed, 1 files unresolved
    use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
    [1]"""
sh % "echo 'merge 2'" > "foo"
sh % "hg resolve -m foo" == "(no more unresolved files)"
sh % "hg ci -m 'Last merge, related'"

sh % "hg log --graph" == r"""
    @    changeset:   10:4dae8563d2c5
    |\   parent:      9:7b35701b003e
    | |  parent:      4:88176d361b69
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     Last merge, related
    | |
    | o    changeset:   9:7b35701b003e
    | |\   parent:      8:e5416ad8a855
    | | |  parent:      7:87fe3144dcfa
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | | |  summary:     First merge, related
    | | |
    | | o  changeset:   8:e5416ad8a855
    | | |  parent:      6:dc6c325fe5ee
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | | |  summary:     change foo in branch, related
    | | |
    | o |  changeset:   7:87fe3144dcfa
    | |/   user:        test
    | |    date:        Thu Jan 01 00:00:00 1970 +0000
    | |    summary:     change foo, related
    | |
    | o  changeset:   6:dc6c325fe5ee
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     create foo, related
    | |
    | o  changeset:   5:73db34516eb9
    | |  parent:      0:e87515fd044a
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     first branch, unrelated
    | |
    o |  changeset:   4:88176d361b69
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     add foo, related
    | |
    o |  changeset:   3:dd78ae4afb56
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     delete foo, unrelated
    | |
    o |  changeset:   2:c4c64aedf0f7
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:00 1970 +0000
    | |  summary:     add unrelated old foo
    | |
    o |  changeset:   1:e5faa7440653
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    summary:     change, unrelated
    |
    o  changeset:   0:e87515fd044a
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     init, unrelated"""

sh % "hg --traceback log -f foo" == r"""
    changeset:   10:4dae8563d2c5
    parent:      9:7b35701b003e
    parent:      4:88176d361b69
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     Last merge, related

    changeset:   9:7b35701b003e
    parent:      8:e5416ad8a855
    parent:      7:87fe3144dcfa
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     First merge, related

    changeset:   8:e5416ad8a855
    parent:      6:dc6c325fe5ee
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     change foo in branch, related

    changeset:   7:87fe3144dcfa
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     change foo, related

    changeset:   6:dc6c325fe5ee
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     create foo, related

    changeset:   4:88176d361b69
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     add foo, related"""

# Also check when maxrev < lastrevfilelog

sh % "hg --traceback log -f -r4 foo" == r"""
    changeset:   4:88176d361b69
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     add foo, related

    changeset:   2:c4c64aedf0f7
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     add unrelated old foo"""
sh % "cd .."

# Issue2383: hg log showing _less_ differences than hg diff

sh % "hg init issue2383"
sh % "cd issue2383"

# Create a test repo:

sh % "echo a" > "a"
sh % "hg ci -Am0" == "adding a"
sh % "echo b" > "b"
sh % "hg ci -Am1" == "adding b"
sh % "hg co 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo b" > "a"
sh % "hg ci -m2"

# Merge:

sh % "hg merge" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

# Make sure there's a file listed in the merge to trigger the bug:

sh % "echo c" > "a"
sh % "hg ci -m3"

# Two files shown here in diff:

sh % "hg diff --rev '2:3'" == r"""
    diff -r b09be438c43a -r 8e07aafe1edc a
    --- a/a	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -b
    +c
    diff -r b09be438c43a -r 8e07aafe1edc b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +b"""

# Diff here should be the same:

sh % "hg log -vpr 3" == r"""
    changeset:   3:8e07aafe1edc
    parent:      2:b09be438c43a
    parent:      1:925d80f479bb
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    files:       a
    description:
    3


    diff -r b09be438c43a -r 8e07aafe1edc a
    --- a/a Thu Jan 01 00:00:00 1970 +0000
    +++ b/a Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -b
    +c
    diff -r b09be438c43a -r 8e07aafe1edc b
    --- /dev/null Thu Jan 01 00:00:00 1970 +0000
    +++ b/b Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +b"""
sh % "cd .."

# 'hg log -r rev fn' when last(filelog(fn)) != rev

sh % "hg init simplelog"
sh % "cd simplelog"
sh % "echo f" > "a"
sh % "hg ci -Ama -d '0 0'" == "adding a"
sh % "echo f" >> "a"
sh % "hg ci '-Ama bis' -d '1 0'"

sh % "hg log -r0 a" == r"""
    changeset:   0:9f758d63dcde
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a"""
# enable obsolete to test hidden feature

sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> "$HGRCPATH"

sh % "hg log '--template={rev}:{node}\\n'" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""
sh % "hg debugobsolete a765632148dc55d38c35c4f247c618701886cb2f" == "obsoleted 1 changesets"
sh % "hg up null -q"
sh % "hg log '--template={rev}:{node}\\n'" == "0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"
sh % "hg log '--template={rev}:{node}\\n' --hidden" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""
sh % "hg log -r a" == r"""
    abort: hidden revision 'a'!
    (use --hidden to access hidden revisions)
    [255]"""

# test that parent prevent a changeset to be hidden

sh % "hg up 1 -q --hidden"
sh % "hg log '--template={rev}:{node}\\n'" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""

# test that second parent prevent a changeset to be hidden too

sh % "hg debugsetparents 0 1"
sh % "hg log '--template={rev}:{node}\\n'" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""
sh % "hg debugsetparents 1"
sh % "hg up -q null"

# bookmarks prevent a changeset being hidden

sh % "hg bookmark --hidden -r 1 X"
sh % "hg log --template '{rev}:{node}\\n'" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""
sh % "hg bookmark -d X"

# divergent bookmarks are not hidden

sh % "hg bookmark --hidden -r 1 'X@foo'"
sh % "hg log --template '{rev}:{node}\\n'" == r"""
    1:a765632148dc55d38c35c4f247c618701886cb2f
    0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05"""

# test hidden revision 0 (issue5385)

sh % "hg bookmark -d 'X@foo'"
sh % "hg up null -q"
sh % "hg debugobsolete 9f758d63dcde62d547ebfb08e1e7ee96535f2b05" == "obsoleted 1 changesets"
sh % "echo f" > "b"
sh % "hg ci -Amb -d '2 0'" == "adding b"
sh % "echo f" >> "b"
sh % "hg ci '-mb bis' -d '3 0'"
sh % "hg log '-T{rev}:{node}\\n'" == r"""
    3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
    2:94375ec45bddd2a824535fc04855bd058c926ec0"""

sh % "hg log '-T{rev}:{node}\\n' '-r:'" == r"""
    2:94375ec45bddd2a824535fc04855bd058c926ec0
    3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e"""
sh % "hg log '-T{rev}:{node}\\n' '-r:tip'" == r"""
    2:94375ec45bddd2a824535fc04855bd058c926ec0
    3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e"""
sh % "hg log '-T{rev}:{node}\\n' '-r:0'" == r"""
    abort: hidden revision '0'!
    (use --hidden to access hidden revisions)
    [255]"""
sh % "hg log '-T{rev}:{node}\\n' -f" == r"""
    3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
    2:94375ec45bddd2a824535fc04855bd058c926ec0"""

# clear extensions configuration
sh % "echo '[extensions]'" >> "$HGRCPATH"
sh % "echo 'obs=!'" >> "$HGRCPATH"
sh % "cd .."


# test hg log on non-existent files and on directories
sh % "newrepo issue1340"
sh % "mkdir d1 D2 D3.i d4.hg d5.d .d6"
sh % "echo 1" > "d1/f1"
sh % "echo 1" > "D2/f1"
sh % "echo 1" > "D3.i/f1"
sh % "echo 1" > "d4.hg/f1"
sh % "echo 1" > "d5.d/f1"
sh % "echo 1" > ".d6/f1"
sh % "hg -q add ."
sh % "hg commit -m 'a bunch of weird directories'"
sh % "hg log -l1 d1/f1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 f1"
sh % "hg log -l1 . -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 ./ -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 d1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 D2 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 D2/f1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 D3.i -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 D3.i/f1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 d4.hg -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 d4.hg/f1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 d5.d -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 d5.d/f1 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 .d6 -T '{node|short}'" == "65624cd9070a"
sh % "hg log -l1 .d6/f1 -T '{node|short}'" == "65624cd9070a"

# issue3772: hg log -r :null showing revision 0 as well

sh % "hg log -r ':null'" == r"""
    changeset:   0:65624cd9070a
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a bunch of weird directories

    changeset:   -1:000000000000
    user:        
    date:        Thu Jan 01 00:00:00 1970 +0000"""
sh % "hg log -r 'null:null'" == r"""
    changeset:   -1:000000000000
    user:         (trailing space)
    date:        Thu Jan 01 00:00:00 1970 +0000"""
# working-directory revision requires special treatment

# clean:

sh % "hg log -r 'wdir()' --debug" == r"""
    changeset:   2147483647:ffffffffffffffffffffffffffffffffffffffff
    phase:       draft
    parent:      0:65624cd9070a035fa7191a54f2b8af39f16b0c08
    parent:      -1:0000000000000000000000000000000000000000
    user:        test
    date:        [A-Za-z0-9:+ ]+ (re)
    extra:       branch=default"""
sh % "hg log -r 'wdir()' -p --stat" == r"""
    changeset:   2147483647:ffffffffffff
    parent:      0:65624cd9070a
    user:        test
    date:        [A-Za-z0-9:+ ]+ (re)"""

# dirty:

sh % "echo 2" >> "d1/f1"
sh % "echo 2" > "d1/f2"
sh % "hg add d1/f2"
sh % "hg remove .d6/f1"
sh % "hg status" == r"""
    M d1/f1
    A d1/f2
    R .d6/f1"""

sh % "hg log -r 'wdir()'" == r"""
    changeset:   2147483647:ffffffffffff
    parent:      0:65624cd9070a
    user:        test
    date:        [A-Za-z0-9:+ ]+ (re)"""
sh % "hg log -r 'wdir()' -q" == "2147483647:ffffffffffff"

sh % "hg log -r 'wdir()' --debug" == r"""
    changeset:   2147483647:ffffffffffffffffffffffffffffffffffffffff
    phase:       draft
    parent:      0:65624cd9070a035fa7191a54f2b8af39f16b0c08
    parent:      -1:0000000000000000000000000000000000000000
    user:        test
    date:        [A-Za-z0-9:+ ]+ (re)
    files:       d1/f1
    files+:      d1/f2
    files-:      .d6/f1
    extra:       branch=default"""
sh % "hg log -r 'wdir()' -p --stat --git" == r"""
    changeset:   2147483647:ffffffffffff
    parent:      0:65624cd9070a
    user:        test
    date:        [A-Za-z0-9:+ ]+ (re)

     .d6/f1 |  1 -
     d1/f1  |  1 +
     d1/f2  |  1 +
     3 files changed, 2 insertions(+), 1 deletions(-)

    diff --git a/.d6/f1 b/.d6/f1
    deleted file mode 100644
    --- a/.d6/f1
    +++ /dev/null
    @@ -1,1 +0,0 @@
    -1
    diff --git a/d1/f1 b/d1/f1
    --- a/d1/f1
    +++ b/d1/f1
    @@ -1,1 +1,2 @@
     1
    +2
    diff --git a/d1/f2 b/d1/f2
    new file mode 100644
    --- /dev/null
    +++ b/d1/f2
    @@ -0,0 +1,1 @@
    +2"""
sh % "hg log -r 'wdir()' -Tjson" == r"""
    [
     {
      "rev": null,
      "node": null,
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [*, 0], (glob)
      "desc": "",
      "bookmarks": [],
      "tags": [],
      "parents": ["65624cd9070a035fa7191a54f2b8af39f16b0c08"]
     }
    ]"""

sh % "hg log -r 'wdir()' -Tjson -q" == r"""
    [
     {
      "rev": null,
      "node": null
     }
    ]"""

sh % "hg log -r 'wdir()' -Tjson --debug" == r"""
    [
     {
      "rev": null,
      "node": null,
      "branch": "default",
      "phase": "draft",
      "user": "test",
      "date": [*, 0], (glob)
      "desc": "",
      "bookmarks": [],
      "tags": [],
      "parents": ["65624cd9070a035fa7191a54f2b8af39f16b0c08"],
      "manifest": null,
      "extra": {"branch": "default"},
      "modified": ["d1/f1"],
      "added": ["d1/f2"],
      "removed": [".d6/f1"]
     }
    ]"""

sh % "hg revert -aqC"

# Check that adding an arbitrary name shows up in log automatically

sh % "cat" << r'''
"""A small extension to test adding arbitrary names to a repo"""
from __future__ import absolute_import
from edenscm.mercurial import namespaces, registrar


namespacepredicate = registrar.namespacepredicate()

@namespacepredicate("bars", priority=70)
def barlookup(repo):
    foo = {'foo': repo[0].node()}
    names = lambda r: foo.keys()
    namemap = lambda r, name: foo.get(name)
    nodemap = lambda r, node: [name for name, n in foo.iteritems()
                               if n == node]
    return namespaces.namespace(
        templatename="bar",
        logname="barlog",
        colorname="barcolor",
        listnames=names,
        namemap=namemap,
        nodemap=nodemap
    )
''' > "../names.py"

sh % "hg --config 'extensions.names=../names.py' log -r 0" == r"""
    changeset:   0:65624cd9070a
    barlog:      foo
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a bunch of weird directories"""
sh % "hg --config 'extensions.names=../names.py' --config 'extensions.color=' --config 'color.log.barcolor=red' '--color=always' log -r 0" == r"""
    [0;33mchangeset:   0:65624cd9070a[0m
    [0;31mbarlog:      foo[0m
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a bunch of weird directories"""
sh % "hg --config 'extensions.names=../names.py' log -r 0 --template '{bars}\\n'" == "foo"

# revert side effect of names.py
from edenscm.mercurial import namespaces

del namespaces.namespacetable["bars"]

# Templater parse errors:

# simple error
sh % "hg log -r . -T '{shortest(node}'" == r"""
    hg: parse error at 15: unexpected token: end
    ({shortest(node}
                   ^ here)
    [255]"""

# multi-line template with error
sh % "hg log -r . -T 'line 1\nline2\n{shortest(node}\nline4\nline5'" == r"""
    hg: parse error at 28: unexpected token: end
    (line 1\nline2\n{shortest(node}\nline4\nline5
                                  ^ here)
    [255]"""

sh % "cd .."

# hg log -f dir across branches

sh % "hg init acrossbranches"
sh % "cd acrossbranches"
sh % "mkdir d"
sh % "echo a" > "d/a"
sh % "hg ci -Aqm a"
sh % "echo b" > "d/a"
sh % "hg ci -Aqm b"
sh % "hg up -q 0"
sh % "echo b" > "d/a"
sh % "hg ci -Aqm c"
sh % "hg log -f d -T '{desc}' -G" == r"""
    @  c
    |
    o  a"""
sh % "hg log -f d -T '{desc}' -G" == r"""
    @  c
    |
    o  a"""
sh % "hg log -f d/a -T '{desc}' -G" == r"""
    @  c
    |
    o  a"""
sh % "cd .."

# hg log -f with linkrev pointing to another branch
# -------------------------------------------------

# create history with a filerev whose linkrev points to another branch

sh % "hg init branchedlinkrev"
sh % "cd branchedlinkrev"
sh % "echo 1" > "a"
sh % "hg commit -Am content1" == "adding a"
sh % "echo 2" > "a"
sh % "hg commit -m content2"
sh % "hg up --rev 'desc(content1)'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo unrelated" > "unrelated"
sh % "hg commit -Am unrelated" == "adding unrelated"
sh % "hg graft -r 'desc(content2)'" == 'grafting 1:2294ae80ad84 "content2"'
sh % "echo 3" > "a"
sh % "hg commit -m content3"
sh % "hg log -G" == r"""
    @  changeset:   4:50b9b36e9c5d
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content3
    |
    o  changeset:   3:15b2327059e5
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content2
    |
    o  changeset:   2:2029acd1168c
    |  parent:      0:ae0a3c9f9e95
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     unrelated
    |
    | o  changeset:   1:2294ae80ad84
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    summary:     content2
    |
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# log -f on the file should list the graft result.

sh % "hg log -Gf a" == r"""
    @  changeset:   4:50b9b36e9c5d
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content3
    |
    o  changeset:   3:15b2327059e5
    :  user:        test
    :  date:        Thu Jan 01 00:00:00 1970 +0000
    :  summary:     content2
    :
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# plain log lists the original version
# (XXX we should probably list both)

sh % "hg log -G a" == r"""
    @  changeset:   4:50b9b36e9c5d
    :  user:        test
    :  date:        Thu Jan 01 00:00:00 1970 +0000
    :  summary:     content3
    :
    : o  changeset:   1:2294ae80ad84
    :/   user:        test
    :    date:        Thu Jan 01 00:00:00 1970 +0000
    :    summary:     content2
    :
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# hg log -f from the grafted changeset
# (The bootstrap should properly take the topology in account)

sh % "hg up 'desc(content3)^'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg log -Gf a" == r"""
    @  changeset:   3:15b2327059e5
    :  user:        test
    :  date:        Thu Jan 01 00:00:00 1970 +0000
    :  summary:     content2
    :
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# Test that we use the first non-hidden changeset in that case.

# (hide the changeset)

sh % "hg log -T '{node}\\n' -r 1" == "2294ae80ad8447bc78383182eeac50cb049df623"
sh % "hg debugobsolete 2294ae80ad8447bc78383182eeac50cb049df623" == "obsoleted 1 changesets"
sh % "hg log -G" == r"""
    o  changeset:   4:50b9b36e9c5d
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content3
    |
    @  changeset:   3:15b2327059e5
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content2
    |
    o  changeset:   2:2029acd1168c
    |  parent:      0:ae0a3c9f9e95
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     unrelated
    |
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# Check that log on the file does not drop the file revision.

sh % "hg log -G a" == r"""
    o  changeset:   4:50b9b36e9c5d
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     content3
    |
    @  changeset:   3:15b2327059e5
    :  user:        test
    :  date:        Thu Jan 01 00:00:00 1970 +0000
    :  summary:     content2
    :
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

# Even when a head revision is linkrev-shadowed.

sh % "hg log -T '{node}\\n' -r 4" == "50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2"
sh % "hg debugobsolete 50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2" == "obsoleted 1 changesets"
sh % "hg log -G a" == r"""
    @  changeset:   3:15b2327059e5
    :  user:        test
    :  date:        Thu Jan 01 00:00:00 1970 +0000
    :  summary:     content2
    :
    o  changeset:   0:ae0a3c9f9e95
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     content1"""

sh % "cd .."

# Even when the file revision is missing from some head:

sh % "hg init issue4490"
sh % "cd issue4490"
sh % "echo '[experimental]'" >> ".hg/hgrc"
sh % "echo 'evolution.createmarkers=True'" >> ".hg/hgrc"
sh % "echo a" > "a"
sh % "hg ci -Am0" == "adding a"
sh % "echo b" > "b"
sh % "hg ci -Am1" == "adding b"
sh % "echo B" > "b"
sh % "hg ci --amend -m 1"
sh % "hg up 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo c" > "c"
sh % "hg ci -Am2" == "adding c"
sh % "hg up 'head() and not .'" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg log -G" == r"""
    o  changeset:   3:db815d6d32e6
    |  parent:      0:f7b1eb17ad24
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     2
    |
    | @  changeset:   2:9bc8ce7f9356
    |/   parent:      0:f7b1eb17ad24
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    summary:     1
    |
    o  changeset:   0:f7b1eb17ad24
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     0"""
sh % "hg log -f -G b" == r"""
    @  changeset:   2:9bc8ce7f9356
    |  parent:      0:f7b1eb17ad24
    ~  user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     1"""
sh % "hg log -G b" == r"""
    @  changeset:   2:9bc8ce7f9356
    |  parent:      0:f7b1eb17ad24
    ~  user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     1"""
sh % "cd .."

# Check proper report when the manifest changes but not the file issue4499
# ------------------------------------------------------------------------

sh % "hg init issue4499"
sh % "cd issue4499"

for f in "ABCDFEGHIJKLMNOPQRSTU":
    sh % "echo 1" > str(f)
sh.hg("add", *list("ABCDFEGHIJKLMNOPQRSTU"))

sh % "hg commit -m A1B1C1"
sh % "echo 2" > "A"
sh % "echo 2" > "B"
sh % "echo 2" > "C"
sh % "hg commit -m A2B2C2"
sh % "hg up 0" == "3 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo 3" > "A"
sh % "echo 2" > "B"
sh % "echo 2" > "C"
sh % "hg commit -m A3B2C2"

sh % "hg log -G" == r"""
    @  changeset:   2:fe5fc3d0eb17
    |  parent:      0:abf4f0e38563
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A3B2C2
    |
    | o  changeset:   1:07dcc6b312c0
    |/   user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    summary:     A2B2C2
    |
    o  changeset:   0:abf4f0e38563
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     A1B1C1"""

# Log -f on B should reports current changesets

sh % "hg log -fG B" == r"""
    @  changeset:   2:fe5fc3d0eb17
    |  parent:      0:abf4f0e38563
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     A3B2C2
    |
    o  changeset:   0:abf4f0e38563
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     A1B1C1"""
sh % "cd .."
