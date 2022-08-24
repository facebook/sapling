#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

# Setup repo

  $ newrepo

# Prepare commits

  >>> import time
  >>> now = int(time.time())
  >>> for delta in [31536000, 86401, 86369, 3800, 420, 5]:
  ...     with open("file1", "wb") as f: f.write(f"{delta}\n".encode()) and None
  ...     cmd = f"hg commit -d '{now-delta} 0' -m 'Changeset {delta} seconds ago' -A file1"
  ...     sheval(cmd) or None

# Check age ranges

  from edenscm.extensions import wrappedfunction
  with wrappedfunction(time, "time", lambda orig: now + 1):
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
