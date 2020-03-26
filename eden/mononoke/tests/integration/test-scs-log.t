# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup config repo:
  $ setup_common_config
  $ cd "$TESTTMP"

Setup testing repo for mononoke:
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

Helper for making commit:
  $ function commit() { # the arg is used both for commit message and variable name
  >   hg commit -qAm $1 -d $2 # create commit with a date
  >   export COMMIT_$1="$(hg --debug id -i)" # save hash to variable
  > }

Commits with dates to test time filters

  $ echo -e "a\nb\nc\nd\ne" > a
  $ commit A "2015-01-01"

  $ echo -e "a\nb\nd\ne\nf" > b
  $ commit B "2016-01-01"

  $ echo -e "b\nc\nd\ne\nf" > b
  $ commit C "2017-01-01"

  $ echo -e "b\nc\nd\ne\nf" > d
  $ commit D "2018-01-01"

  $ echo -e "p\nk\nt\ne\nf" > b
  $ commit E "2019-01-01"

  $ echo -e "b\nn\ne\nw\nl" > b
  $ commit F "2020-01-01"

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"

Test log
  $ scsc log --repo repo -i "$COMMIT_C" --path b 
  Commit: dba4093ee164cd983101ff9e37751e2a5465c6a9
  Parent: 88619a661eeda8bca794249b9852c83dacada01a
  Date: 2017-01-01 00:00:00 +00:00
  Author: test
  Generation: 3
  
  C
  
  Commit: 88619a661eeda8bca794249b9852c83dacada01a
  Parent: 7b9ef4e3cdfffb431178532bb75534439c14c014
  Date: 2016-01-01 00:00:00 +00:00
  Author: test
  Generation: 2
  
  B
  
  $ scsc --json log --repo repo -i "$COMMIT_F" --path b --limit 2 | jq -S .
  [
    {
      "author": "test",
      "date": "2020-01-01T00:00:00+00:00",
      "extra": {},
      "extra_hex": {},
      "generation": 6,
      "ids": {
        "bonsai": "c695b75bf2396285cec024a7c63dcffef19d9ba3aaa902f409c0bdbb6d268414",
        "hg": "3a61e10442a9b76f8826b05e7ef1a60d33c3bc2d"
      },
      "message": "F",
      "parents": [
        {
          "bonsai": "175af9015b2ca0b133689b73de2ae4e6c34892d62a3c5614ad7170efb0b475fa",
          "hg": "ecbf21bc13d7ec53c820078066ca1dfeb1e8191d"
        }
      ],
      "timestamp": 1577836800,
      "timezone": 0,
      "type": "commit"
    },
    {
      "author": "test",
      "date": "2019-01-01T00:00:00+00:00",
      "extra": {},
      "extra_hex": {},
      "generation": 5,
      "ids": {
        "bonsai": "175af9015b2ca0b133689b73de2ae4e6c34892d62a3c5614ad7170efb0b475fa",
        "hg": "ecbf21bc13d7ec53c820078066ca1dfeb1e8191d"
      },
      "message": "E",
      "parents": [
        {
          "bonsai": "4e1e8b4466f38fc8f37fe637d47edf0953d4d4d289813bf89b6ff7ff092638f2",
          "hg": "aaff25985c53d3ba33e0292b2a271c6cdf34e521"
        }
      ],
      "timestamp": 1546300800,
      "timezone": 0,
      "type": "commit"
    }
  ]

  $ scsc log --repo repo -i "$COMMIT_F" --path b --limit 1 --skip 1
  Commit: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Parent: aaff25985c53d3ba33e0292b2a271c6cdf34e521
  Date: 2019-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E
  
log between 2017/01/01 and 2019/05/05
  $ scsc log --repo repo -i "$COMMIT_F" --path b --after 1483228800 --before 1557061200
  Commit: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Parent: aaff25985c53d3ba33e0292b2a271c6cdf34e521
  Date: 2019-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E
  
  Commit: dba4093ee164cd983101ff9e37751e2a5465c6a9
  Parent: 88619a661eeda8bca794249b9852c83dacada01a
  Date: 2017-01-01 00:00:00 +00:00
  Author: test
  Generation: 3
  
  C
  
  $ scsc log --repo repo -i "$COMMIT_F" --path b --after "2017-01-01 00:00:00" --before "2019-05-05 13:00:00"
  Commit: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Parent: aaff25985c53d3ba33e0292b2a271c6cdf34e521
  Date: 2019-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E
  
  Commit: dba4093ee164cd983101ff9e37751e2a5465c6a9
  Parent: 88619a661eeda8bca794249b9852c83dacada01a
  Date: 2017-01-01 00:00:00 +00:00
  Author: test
  Generation: 3
  
  C
  

log check the timezone parsing
  $ scsc log --repo repo -i "$COMMIT_F" --path b --after "2019-01-01 05:00:00 +08:00"
  Commit: 3a61e10442a9b76f8826b05e7ef1a60d33c3bc2d
  Parent: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Date: 2020-01-01 00:00:00 +00:00
  Author: test
  Generation: 6
  
  F
  
  Commit: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Parent: aaff25985c53d3ba33e0292b2a271c6cdf34e521
  Date: 2019-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E
  
log for the "zero" timestamp
  $ scsc log --repo repo -i "$COMMIT_F" --path b --before 0 --limit 1
  error: The given date or timestamp must be after 1970-01-01 00:00:00 UTC: "0"
  [1]
  $ scsc log --repo repo -i "$COMMIT_F" --path b --after "1969-01-01 00:00:00" --limit 1
  error: The given date or timestamp must be after 1970-01-01 00:00:00 UTC: "1969-01-01 00:00:00"
  [1]

log skip and time filters conflict
  $ scsc log --repo repo -i "$COMMIT_F" --path b --after "2017-01-01 05:00:00 +08:00" --skip 5
  error: The argument '--skip <SKIP>' cannot be used with '--after <AFTER>'
  
  USAGE:
      scsc <--tier <TIER>|--host <HOST:PORT>> log --after <AFTER> --limit <LIMIT> --path <PATH> --repo <REPO> --schemes <SCHEMES>... --skip <SKIP> <--commit-id <COMMIT_ID>|--bookmark <BOOKMARK>|--hg-commit-id <HG_COMMIT_ID>|--bonsai-id <BONSAI_ID>|--git <GIT_SHA1>|--globalrev <GLOBALREV>>
  
  For more information try --help
  [1]

log request a single commit
  $ scsc log --repo repo -i "$COMMIT_F" --path b --limit 1
  Commit: 3a61e10442a9b76f8826b05e7ef1a60d33c3bc2d
  Parent: ecbf21bc13d7ec53c820078066ca1dfeb1e8191d
  Date: 2020-01-01 00:00:00 +00:00
  Author: test
  Generation: 6
  
  F
  
