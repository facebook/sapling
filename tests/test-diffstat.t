  $ hg init repo
  $ cd repo
  $ i=0; while [ "$i" -lt 213 ]; do echo a >> a; i=`expr $i + 1`; done
  $ hg add a
  $ cp a b
  $ hg add b

Wide diffstat:

  $ hg diff --stat
   a |  213 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
   b |  213 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
   2 files changed, 426 insertions(+), 0 deletions(-)

diffstat width:

  $ COLUMNS=24 hg diff --config ui.interactive=true --stat
   a |  213 ++++++++++++++
   b |  213 ++++++++++++++
   2 files changed, 426 insertions(+), 0 deletions(-)

  $ hg ci -m adda

  $ cat >> a <<EOF
  > a
  > a
  > a
  > EOF

Narrow diffstat:

  $ hg diff --stat
   a |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)

  $ hg ci -m appenda

  >>> open("c", "wb").write("\0")
  $ touch d
  $ hg add c d

Binary diffstat:

  $ hg diff --stat
   c |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

Binary git diffstat:

  $ hg diff --stat --git
   c |  Bin 
   d |    0 
   2 files changed, 0 insertions(+), 0 deletions(-)

  $ hg ci -m createb

  >>> open("file with spaces", "wb").write("\0")
  $ hg add "file with spaces"

Filename with spaces diffstat:

  $ hg diff --stat
   file with spaces |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

Filename with spaces git diffstat:

  $ hg diff --stat --git
   file with spaces |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

  $ cd ..
