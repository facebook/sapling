#chg-compatible

  $ configure modernclient
  $ newclientrepo
  $ setconfig sampling.filepath=$TESTTMP/sample sampling.key.command_info=my_cat

  $ hg st
  $ hg st --modified -X '**.go' -X '**.rs'
  $ hg st --no-root-relative --quiet --pager=never -I ''
  $ hg files --prin -X abc -X def || true

  >>> import json
  >>> with open(r"$TESTTMP/sample", mode="rb") as f:
  ...     data = f.read()
  >>> record_count = 0
  >>> for record in data.strip(b"\0").split(b"\0"):
  ...     record = json.loads(record)
  ...     if record['category'] == "my_cat":
  ...         for k in ["option_names", "option_values", "positional_args"]:
  ...             if k in record["data"]:
  ...                 if record_count % 3 == 0:
  ...                     print()
  ...                 record_count += 1
  ...                 print("%s: %s" % (k, record["data"][k]))
  
  positional_args: ['st']
  option_names: []
  option_values: []
  
  positional_args: ['st']
  option_names: ['modified', 'exclude']
  option_values: [True, ['**.go', '**.rs']]
  
  positional_args: ['st']
  option_names: ['root-relative', 'quiet', 'pager', 'include']
  option_values: [False, True, 'never', ['']]
  
  option_names: ['print0', 'exclude']
  option_values: [True, ['abc', 'def']]
  positional_args: ['files']
