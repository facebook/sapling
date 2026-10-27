# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Write one synthetic corpus case to a directory for manual replay.

`buck2 run fbcode//eden/integration/correctness/exploratory:corpus_export -- <family>/<case> <dir>`
writes the case's `run-spec.json`, `history.jsonl`, and `expected.json`.
Nothing reads the written files back: the corpus is rendered in memory
(`loader.cases()`).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from eden.integration.correctness.exploratory.test.corpus import families


def export(name: str, directory: Path) -> None:
    """Write case `name` (`<family>/<case>`) under `directory`."""
    matches = [run for run in families.runs() if f"{run.family}/{run.name}" == name]
    if len(matches) != 1:
        raise ValueError(f"no corpus case {name!r}")
    (run,) = matches
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "run-spec.json").write_text(run.spec_text(), encoding="utf-8")
    (directory / "history.jsonl").write_bytes(run.history_bytes())
    (directory / "expected.json").write_text(
        json.dumps(run.expectation(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    name, directory = sys.argv[1:]
    export(name, Path(directory))


if __name__ == "__main__":
    main()
