
  $ enable fbcodereview
  $ newclientrepo

Control: a reviewer list with no token containing "reviewer" renders.

  $ echo a > a
  $ hg commit -Aqm "commit a
  > 
  > Reviewers: alice, bob #foo"
  $ hg log -r . -T '{reviewers}\n'
  alice bob #foo
  $ hg log -r . -T '{reviewers % "- {reviewer}\n"}'
  - alice
  - bob
  - #foo

A reviewer token that contains the substring "reviewer" (a Phabricator group
such as #foo_reviewers) crashes "{reviewers}".

  $ echo b > b
  $ hg commit -Aqm "commit b
  > 
  > Reviewers: alice #foo_reviewers"
  $ hg log -r . -T '{reviewers}\n' 2>&1 | grep -E '^(TypeError|alice)'
  TypeError: 'str' object is not callable
  $ hg log -r . -T '{reviewers % "- {reviewer}\n"}' 2>&1 | grep -E '^(TypeError|- )'
  - alice
  - #foo_reviewers
