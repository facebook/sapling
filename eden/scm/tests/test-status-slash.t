  $ newclientrepo repo
  $ mkdir dir
  $ touch dir/file

    >>> import os, subprocess
    >>> bad = []
    >>> for root_relative in [True, False]:
    ...    for ui_slash in [True, False]:
    ...        for use_rust in [True, False]:
    ...            for plain in [True, False]:
    ...                got = sheval(" ".join([
    ...                    "HGPLAIN=1" if plain else "",
    ...                    "hg", "status", "--no-status",
    ...                    "--" + ("no-" if not root_relative else "") + "root-relative",
    ...                    "--config", f"ui.slash={ui_slash}",
    ...                    "--config", f"status.use-rust={use_rust}"
    ...                ])).strip()
    ...                want = "dir/file"
    ...                if os.name == "nt" and not ui_slash and not plain:
    ...                    want = r"dir\file"
    ...                if got != want:
    ...                    bad.append(f"got {got}, wanted {want} for root_relative={root_relative} ui_slash={ui_slash} use_rust={use_rust} plain={plain}")
    >>> bad
    []
