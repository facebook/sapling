Validate the flag combinations for `sl web` TLS/binding options. These all
abort during option validation, before any server is spawned.

  $ newclientrepo

--bind alone is rejected: a non-local binding would send the auth token in
plain text without TLS.

  $ sl web --bind all --no-open
  abort: --bind requires --cert and --key, so the auth token is not sent in plain text to other hosts
  [255]

--cert and --key must come as a pair (the pair check fires first even when
--bind is also missing its TLS config).

  $ sl web --cert /path/to/cert.pem --no-open
  abort: --cert and --key must be used together
  [255]

  $ sl web --key /path/to/key.pem --no-open
  abort: --cert and --key must be used together
  [255]

  $ sl web --bind all --cert /path/to/cert.pem --no-open
  abort: --cert and --key must be used together
  [255]

  $ sl web --bind all --key /path/to/key.pem --no-open
  abort: --cert and --key must be used together
  [255]
