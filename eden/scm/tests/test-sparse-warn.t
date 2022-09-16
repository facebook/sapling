#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig workingcopy.ruststatus=False

  $ enable sparse
  $ newrepo
  $ touch file
  $ hg commit -Aqm 'add file'

  $ setconfig sparse.warnfullcheckout=hint
  $ hg status
  hint[sparse-fullcheckout]: warning: full checkouts will eventually be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository.
  hint[hint-ack]: use 'hg hint --ack sparse-fullcheckout' to silence these hints

  $ setconfig sparse.warnfullcheckout=warn
  $ hg status
  warning: full checkouts will soon be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository.

  $ setconfig sparse.warnfullcheckout=softblock
  $ hg status
  abort: full checkouts are not supported for this repository
  (use EdenFS or hg sparse)
  [255]

  $ setconfig sparse.bypassfullcheckoutwarn=True
  $ hg status
  warning: full checkouts will soon be disabled in this repository. Use EdenFS or hg sparse to get a smaller repository.

  $ setconfig sparse.warnfullcheckout=hardblock
  $ hg status
  abort: full checkouts are not supported for this repository
  (use EdenFS or hg sparse)
  [255]

  $ hg sparse include file
  $ hg status
