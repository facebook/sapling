#chg-compatible

  $ setconfig config.use-rust=True
  $ setconfig workingcopy.use-rust=False

Test config:
  $ setconfig testsection.subsection1=foo
  $ setconfig testsection.subsection2=bar
  $ hg --config foo.bar=baz config testsection
  testsection.subsection1=foo
  testsection.subsection2=bar
  $ hg --config foo.bar=baz config foo -Tjson
  [
  {
    "name": "foo.bar",
    "source": "--config",
    "value": "baz"
  }
  ]
  $ hg --config foo.bar=baz config foo -Tdebug
  config = [
      {'source': '--config', 'name': 'foo.bar', 'value': 'baz'},
  ]
  $ hg --config foo.bar=baz config foo.bar
  baz
  $ hg --config foo.bar=baz config foo.bar -Tjson
  [
  {
    "name": "foo.bar",
    "source": "--config",
    "value": "baz"
  }
  ]
  $ hg --config foo.bar=baz config foo.bar -Tdebug
  config = [
      {'source': '--config', 'value': 'baz', 'name': 'foo.bar'},
  ]
  $ hg config testsection
  testsection.subsection1=foo
  testsection.subsection2=bar
  $ hg config testsection --debug
  *.hgrc:*: testsection.subsection1=foo (glob)
  *.hgrc:*: testsection.subsection2=bar (glob)
  $ hg config testsection -Tdebug
  config = [
      {'source': '*.hgrc:*', 'name': 'testsection.subsection1', 'value': 'foo'}, (glob)
      {'source': '*.hgrc:*', 'name': 'testsection.subsection2', 'value': 'bar'}, (glob)
  ]
  $ hg config testsection -Tjson
  [
  {
    "name": "testsection.subsection1",
    "source": "*.hgrc:*", (glob)
    "value": "foo"
  },
  {
    "name": "testsection.subsection2",
    "source": "*.hgrc:*", (glob)
    "value": "bar"
  }
  ]
  $ hg config testsection.subsection1
  foo
  $ hg config testsection.subsection1 --debug
  *.hgrc:* foo (glob)
  $ hg config testsection.subsection1 -Tdebug
  config = [
      {'source': '*.hgrc:*', 'value': 'foo', 'name': 'testsection.subsection1'}, (glob)
  ]
  $ hg config testsection.subsection1 -Tjson
  [
  {
    "name": "testsection.subsection1",
    "source": "*.hgrc:*", (glob)
    "value": "foo"
  }
  ]

Test status:
  $ hg init testrepo
  $ cd testrepo
  $ touch file1
  $ touch file2
  $ hg status
  ? file1
  ? file2
  $ hg status -Tdebug
  status = [
      {'status': '?', 'path': 'file1'},
      {'status': '?', 'path': 'file2'},
  ]
  $ hg status -Tjson
  [
   {
    "path": "file1",
    "status": "?"
   },
   {
    "path": "file2",
    "status": "?"
   }
  ]
