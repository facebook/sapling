---
sidebar_position: 40
---

## ssl
<!--
  @generated SignedSource<<adf1085c88ed3f8539fddfae00bc77f8>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


show a graph of your commits and associated GitHub pull request status

`ssl` is not a subcommand, but a built-in alias for `smartlog -T {ssl}`.
If you have used Sapling to create pull requests for your commits, then
you can use `sl ssl` to include the pull request status in your Smartlog:

```
$ sl ssl
  @  4d9180fd8  6 minutes ago  alyssa  #178 Unreviewed
  │  adding baz
  │
  o  3cc43c835  6 minutes ago  alyssa  #177 Approved
  │  adding bar
  │
  o  4f1243a8b  6 minutes ago  alyssa  #176 Closed
╭─╯  adding foo
│
o  f22585511  Oct 06 at 17:40  remote/main
│
~
```


