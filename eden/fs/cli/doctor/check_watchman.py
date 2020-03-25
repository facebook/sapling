#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import io
import json
import logging
import subprocess
import typing
from typing import Any, Dict, List, Optional, Set

from eden.cli.doctor.problem import (
    FixableProblem,
    Problem,
    ProblemTracker,
    RemediationError,
)


log = logging.getLogger("eden.cli.doctor.checks.watchman")


WatchmanCheckInfo = collections.namedtuple(
    "WatchmanCheckInfo", ["watchman_roots", "nuclide_roots"]
)


def pre_check() -> WatchmanCheckInfo:
    watchman_roots = _get_watch_roots_for_watchman()
    nuclide_roots = _get_roots_for_nuclide()
    return WatchmanCheckInfo(watchman_roots, nuclide_roots)


def check_active_mount(
    tracker: ProblemTracker, path: str, info: WatchmanCheckInfo
) -> None:
    check_watchman_subscriptions(tracker, path, info)
    check_nuclide_subscriptions(tracker, path, info)


class IncorrectWatchmanWatch(FixableProblem):
    def __init__(self, path: str, watcher: Any) -> None:
        self._path = path
        self._watcher = watcher

    def description(self) -> str:
        return (
            f"Watchman is watching {self._path} with the wrong watcher type: "
            f'"{self._watcher}" instead of "eden"'
        )

    def dry_run_msg(self) -> str:
        return f"Would fix watchman watch for {self._path}"

    def start_msg(self) -> str:
        return f"Fixing watchman watch for {self._path}"

    def perform_fix(self) -> None:
        # Delete the old watch and try to re-establish it. Hopefully it will be
        # an Eden watch this time.
        _call_watchman(["watch-del", self._path])
        watch_details = _call_watchman(["watch-project", self._path])
        if watch_details.get("watcher") != "eden":
            raise RemediationError(
                f"Failed to replace watchman watch for {self._path} "
                'with an "eden" watcher'
            )


def check_watchman_subscriptions(
    tracker: ProblemTracker, path: str, info: WatchmanCheckInfo
) -> None:
    if path not in info.watchman_roots:
        return

    watch_details = _call_watchman(["watch-project", path])
    watcher = watch_details.get("watcher")
    if watcher == "eden":
        return

    tracker.add_problem(IncorrectWatchmanWatch(path, watcher))


# Watchman subscriptions that Nuclide creates for an Hg repository.
NUCLIDE_HG_SUBSCRIPTIONS = [
    "hg-repository-watchman-subscription-primary",
    "hg-repository-watchman-subscription-conflicts",
    "hg-repository-watchman-subscription-hgbookmark",
    "hg-repository-watchman-subscription-hgbookmarks",
    "hg-repository-watchman-subscription-dirstate",
    "hg-repository-watchman-subscription-progress",
    "hg-repository-watchman-subscription-lock-files",
]


def check_nuclide_subscriptions(
    tracker: ProblemTracker, path: str, info: WatchmanCheckInfo
) -> None:
    if info.nuclide_roots is None:
        return

    # Note that nuclide_roots is a set, but each entry in the set
    # could appear as a root folder multiple times if the user uses multiple
    # Atom windows.
    path_prefix = path + "/"
    connected_nuclide_roots = [
        nuclide_root
        for nuclide_root in info.nuclide_roots
        if path == nuclide_root or nuclide_root.startswith(path_prefix)
    ]
    if not connected_nuclide_roots:
        # There do not appear to be any Nuclide connections for path.
        return

    subscriptions = _call_watchman(["debug-get-subscriptions", path])
    subscribers = subscriptions.get("subscribers", [])
    subscription_counts: Dict[str, int] = {}
    for subscriber in subscribers:
        subscriber_info = subscriber.get("info", {})
        name = subscriber_info.get("name")
        if name is None:
            continue
        elif name in subscription_counts:
            subscription_counts[name] += 1
        else:
            subscription_counts[name] = 1

    missing_or_duplicate_subscriptions = []
    for nuclide_root in connected_nuclide_roots:
        filewatcher_subscription = f"filewatcher-{nuclide_root}"
        # Note that even if the user has `nuclide_root` opened in multiple
        # Nuclide windows, the Nuclide server should not create the
        # "filewatcher-" subscription multiple times.
        if subscription_counts.get(filewatcher_subscription) != 1:
            missing_or_duplicate_subscriptions.append(filewatcher_subscription)

    # Today, Nuclide creates a number of Watchman subscriptions per root
    # folder that is under an Hg working copy. (It should probably
    # consolidate these subscriptions, though it will take some work to
    # refactor things to do that.) Because each of connected_nuclide_roots
    # is a root folder in at least one Atom window, there must be at least
    # as many instances of each subscription as there are
    # connected_nuclide_roots.
    #
    # TODO(mbolin): Come up with a more stable contract than including a
    # hardcoded list of Nuclide subscription names in here because Eden and
    # Nuclide releases are not synced. This is admittedly a stopgap measure:
    # the primary objective is to figure out how Eden/Nuclide gets into
    # this state to begin with and prevent it.
    #
    # Further, Nuclide should probably rename these subscriptions so that:
    # (1) It is clear that Nuclide is the one who created the subscription.
    # (2) The subscription can be ascribed to an individual Nuclide client
    #     if we are going to continue to create the same subscription
    #     multiple times.
    num_roots = len(connected_nuclide_roots)
    for hg_subscription in NUCLIDE_HG_SUBSCRIPTIONS:
        if subscription_counts.get(hg_subscription, 0) < num_roots:
            missing_or_duplicate_subscriptions.append(hg_subscription)

    if missing_or_duplicate_subscriptions:

        def format_paths(paths: List[str]) -> str:
            return "\n  ".join(paths)

        missing_subscriptions = [
            sub
            for sub in missing_or_duplicate_subscriptions
            if 0 == subscription_counts.get(sub, 0)
        ]
        duplicate_subscriptions = [
            sub
            for sub in missing_or_duplicate_subscriptions
            if 1 < subscription_counts.get(sub, 0)
        ]

        output = io.StringIO()
        output.write(
            "Nuclide appears to be used to edit the following directories\n"
            f"under {path}:\n\n"
            f"  {format_paths(connected_nuclide_roots)}\n\n"
        )
        if missing_subscriptions:
            output.write(
                "but the following Watchman subscriptions appear to be missing:\n\n"
                f"  {format_paths(missing_subscriptions)}\n\n"
            )
        if duplicate_subscriptions:
            conj = "and" if missing_subscriptions else "but"
            output.write(
                f"{conj} the following Watchman subscriptions have duplicates:\n\n"
                f"  {format_paths(duplicate_subscriptions)}\n\n"
            )
        output.write(
            "This can cause file changes to fail to show up in Nuclide.\n"
            "Currently, the only workaround for this is to run\n"
            '"Nuclide Remote Projects: Kill And Restart" from the\n'
            "command palette in Atom.\n"
        )
        tracker.add_problem(Problem(output.getvalue()))


def _get_watch_roots_for_watchman() -> Set[str]:
    js = _call_watchman(["watch-list"])
    roots = set(js.get("roots", []))
    return roots


def _call_watchman(args: List[str]) -> Dict:
    full_args = ["watchman"]
    full_args.extend(args)
    return _check_json_output(full_args)


def _get_roots_for_nuclide() -> Optional[Set[str]]:
    connections = _check_json_output(["nuclide-connections"])
    if isinstance(connections, list):
        return set(connections)
    else:
        # connections should be a dict with an "error" property.
        return None


def _check_json_output(args: List[str]) -> Dict[str, Any]:
    """Calls subprocess.check_output() and returns the output parsed as JSON.
    If the call fails, it will write the error to stderr and return a dict with
    a single property named "error".
    """
    try:
        output = subprocess.check_output(args)
        return typing.cast(Dict[str, Any], json.loads(output))
    except FileNotFoundError as e:
        # Same as below, but we don't need to emit a warning if they don't have
        # nuclide-connections installed.
        errstr = getattr(e, "strerror", str(e))
        return {"error": str(e)}
    except Exception as e:
        # FileNotFoundError if the command is not found.
        # CalledProcessError if the command exits unsuccessfully.
        # ValueError if `output` is not valid JSON.
        errstr = getattr(e, "strerror", str(e))
        log.warning(f'Calling `{" ".join(args)}` failed with: {errstr}')
        return {"error": str(e)}
