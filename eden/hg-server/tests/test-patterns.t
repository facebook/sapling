#require no-windows

Explore the semi-mysterious matchmod.match API

  $ newrepo
  $ mkdir 'a*1' 'a*2'
  $ touch 'a*1/a' 'a*2/b'
  $ hg ci -m 1 -A 'a*1/a' 'a*2/b' -q 2>&1 | sort
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
