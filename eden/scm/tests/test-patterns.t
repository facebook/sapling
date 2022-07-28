#require no-windows

Explore the semi-mysterious matchmod.match API

  $ newrepo
  $ mkdir 'a*1' 'a*2'
  $ touch 'a*1/a' 'a*2/b'
  $ hg ci -m 1 -A 'a*1/a' 'a*2/b' -q 2>&1 | sort
  possible glob in non-glob pattern: a*1/a
  possible glob in non-glob pattern: a*2/b
  warning: filename contains '*', which is reserved on Windows: 'a*1/a'
  warning: filename contains '*', which is reserved on Windows: 'a*2/b'

"patterns="

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*"])))))'
  []

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*1"])))))'
  []

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*/*"])))))'
  ['a*1/a', 'a*2/b']

"include="

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*"])))))'
  ['a*1/a', 'a*2/b']

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*1"])))))'
  ['a*1/a']

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*/*"])))))'
  ['a*1/a', 'a*2/b']

"patterns=" with "default='path'"

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*"], default="path")))))'
  []

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*1"], default="path")))))'
  ['a*1/a']

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", patterns=["a*/*"], default="path")))))'
  []

"include=" with "default='path'" (ex. "default=" has no effect on "include=")

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*"], default="path")))))'
  ['a*1/a', 'a*2/b']

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*1"], default="path")))))'
  ['a*1/a']

  $ hg dbsh -c 'ui.write("%s\n" % str(list(repo["."].walk(m.match.match(repo.root, "", include=["a*/*"], default="path")))))'
  ['a*1/a', 'a*2/b']

Give a hint if a pattern will traverse the entire repo.
  $ hg files 'glob:**/*.cpp' --config hint.ack-match-full-traversal=false
  hint[match-full-traversal]: the patterns "glob:**/*.cpp" may be slow since they traverse the entire repo (see "hg help patterns")
  [1]

No hint since the prefix avoids the full traversal.
  $ hg files 'glob:foo/**/*.cpp' --config hint.ack-match-full-traversal=false
  [1]

No hint when run from a sub-directory since it won't traverse the entire repo.
  $ mkdir foo
  $ cd foo
  $ hg files 'glob:**/*.cpp' --config hint.ack-match-full-traversal=false
  [1]
