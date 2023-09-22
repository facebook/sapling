#chg-compatible

  $ configure modernclient
  $ newclientrepo
  $ setconfig sampling.filepath=$TESTTMP/sample sampling.key.command_info=my_cat

  $ hg st
  $ hg st --modified -X '**.go' -X '**.rs'
  $ hg st --no-root-relative --quiet --pager=never -I ''

  >>> import json
  >>> with open(r"$TESTTMP/sample", mode="rb") as f:
  ...     data = f.read()
  >>> for record in data.strip(b"\0").split(b"\0"):
  ...     record = json.loads(record)
  ...     if record['category'] == "my_cat":
  ...         for k in ["option_names", "option_values", "positional_args"]:
  ...             if k in record["data"]:
  ...                 if k == "positional_args":
  ...                     print("\n")
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
