
  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > age=
  > EOF

Setup repo
  $ hg init repo
  $ cd repo
  $ now=`date +%s`
  $ touch file1
  $ hg add file1
  $ for delta in 31536000 86401 86369 3800 420 5
  > do
  >   commit_time=`expr $now - $delta`
  >   echo "$delta" > file1
  >   hg commit -d "$commit_time 0" -m "Changeset $delta seconds ago"
  > done

Check age ranges
  $ hg log -T '{rev} {desc}\n' -r 'age("<30")'
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<7m30s")'
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<1h4m")'
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<1d")'
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<364d23h59m")'
  1 Changeset 86401 seconds ago
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age(">1s")'
  0 Changeset 31536000 seconds ago
  1 Changeset 86401 seconds ago
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age(">1m")'
  0 Changeset 31536000 seconds ago
  1 Changeset 86401 seconds ago
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age(">1h")'
  0 Changeset 31536000 seconds ago
  1 Changeset 86401 seconds ago
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age(">1d")'
  0 Changeset 31536000 seconds ago
  1 Changeset 86401 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age(">365d")'
  0 Changeset 31536000 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<64m")'
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<60m500s")'
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("<1h500s")'
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("1h-20d")'
  1 Changeset 86401 seconds ago
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'age("invalid")'
  hg: parse error: invalid age range
  [255]
  $ hg log -T '{rev} {desc}\n' -r 'age("1h")'
  hg: parse error: invalid age range
  [255]
  $ hg log -T '{rev} {desc}\n' -r 'age("<3m2h")'
  hg: parse error: invalid age in age range: 3m2h
  [255]
  $ hg log -T '{rev} {desc}\n' -r 'age(">3h2h")'
  hg: parse error: invalid age in age range: 3h2h
  [255]
  $ hg log -T '{rev} {desc}\n' -r 'age("1h-5h-10d")'
  hg: parse error: invalid age in age range: 5h-10d
  [255]
  $ hg log -T '{rev} {desc}\n' -r 'ancestorsaged(., "<1d")'
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  5 Changeset 5 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'ancestorsaged(.^, "<1d")'
  2 Changeset 86369 seconds ago
  3 Changeset 3800 seconds ago
  4 Changeset 420 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'ancestorsaged(., "1d-20d")'
  1 Changeset 86401 seconds ago
  $ hg log -T '{rev} {desc}\n' -r 'ancestorsaged(., ">1d")'
  0 Changeset 31536000 seconds ago
  1 Changeset 86401 seconds ago
