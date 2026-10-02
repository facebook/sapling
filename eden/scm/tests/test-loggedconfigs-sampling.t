#inprocess-hg-incompatible

  $ setconfig sampling.filepath=$TESTTMP/sample sampling.key.logginghelper=my_cat
  $ setconfig loggedconfigs.foo=my.foo loggedconfigs.unset=my.unset loggedconfigs.bad=my.a.b
  $ setconfig my.foo=bar my.a.b=baz

  >>> import json, os
  >>> def logged():
  ...     with open(os.path.join(os.environ["TESTTMP"], "sample"), mode="rb") as f:
  ...         records = [json.loads(r) for r in f.read().strip(b"\0").split(b"\0")]
  ...     return [r["data"] for r in records if r["category"] == "my_cat"]

Every command logs [loggedconfigs] exactly once, with or without a repo:

  $ sl version -q > /dev/null
  >>> logged()
  [{'foo': 'bar'}]

  $ newclientrepo
  $ rm $TESTTMP/sample
  $ echo a > a
  $ sl commit -Aqm a
  >>> logged()
  [{'foo': 'bar'}]
