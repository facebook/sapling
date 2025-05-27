#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import re
import subprocess
from pathlib import Path

from typing import List

from eden.fs.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker
from eden.fs.cli.util import get_environment_suitable_for_subprocess

try:
    from .facebook.internal_consts import get_netinfo_link
except ImportError:

    def get_netinfo_link() -> str:
        return ""


NETWORK_TIMEOUT = 15.0
MIN_DOWNLOAD_SPEED = 50.0
MIN_UPLOAD_SPEED = 10.0


class NetworkSpeedProblem(Problem):
    def __init__(self, output: str) -> None:
        super().__init__(
            f"Failed to verify speed of connection to eden services: {output}",
            remediation="Check the speed report in hg debugnetwork --speed",
        )


class NetworkSlowSpeedProblem(Problem):
    def __init__(self, speeds: list[float]) -> None:
        super().__init__(
            f"Slow network speed detected: {form_network_speed_message(speeds[0], speeds[1])}",
            severity=ProblemSeverity.POTENTIALLY_SERIOUS,
            remediation=f"Please check if anything is consuming an excess amount of bandwidth on your network.{get_netinfo_link()}",
        )


class NetworkLatencyProblem(Problem):
    def __init__(self, latency: float) -> None:
        super().__init__(
            f"High network latency detected: Latency {latency} ms higher than 250ms",
            severity=ProblemSeverity.POTENTIALLY_SERIOUS,
            remediation=f"Please check if anything is causing high ping on your network.{get_netinfo_link()}",
        )


class ConnectivityProblem(Problem):
    def __init__(self, errmsg: str) -> None:
        super().__init__(
            f"Encountered an error checking connection to Source Control Servers: {errmsg}",
            remediation="Please check your network connection. If you are connected to the VPN, please try reconnecting.",
            severity=ProblemSeverity.ERROR,
        )


def form_network_speed_message(download_speed: float, upload_speed: float) -> str:
    slow_download = download_speed < MIN_DOWNLOAD_SPEED
    slow_upload = upload_speed < MIN_UPLOAD_SPEED
    if slow_download and slow_upload:
        return f"Average download speed {download_speed:.2f} Mbit/s slower than {int(MIN_DOWNLOAD_SPEED)} Mbit/s, and average upload speed {upload_speed:.2f}Mbit/s slower than {int(MIN_UPLOAD_SPEED)} Mbit/s"
    if slow_download:
        return f"Average download speed {download_speed:.2f} Mbit/s slower than {int(MIN_DOWNLOAD_SPEED)} Mbit/s"
    else:
        return f"Average upload speed {upload_speed:.2f} Mbit/s slower than {int(MIN_UPLOAD_SPEED)} Mbit/s"


def parse_latency(latency: str) -> float:
    # Latency is printed as a value of 4 digits in the closest order of magnitue separated by a space
    # e.g. 1000 s, 100.0 s, 10.00s, 1.000s, 100.0 ms, ... 1.000 us
    # We want to convert this to milliseconds
    magnitude = {"s": 1000, "ms": 1, "us": 1.0 / 1000.0}
    (value, unit) = latency.split(" ")
    return float(value) * magnitude[unit]


def fmtProblemMessage(
    description: str, ex: subprocess.TimeoutExpired | subprocess.CalledProcessError
) -> str:
    def fmtExcMsg(errmsg: str | bytes | None) -> str:
        if errmsg is None:
            return "None"
        elif type(errmsg) is str:
            return errmsg
        else:
            decoded = errmsg.decode()
            if len(decoded) == 0:
                return "None"
            return decoded

    return f"{description}\nStdout:\n{fmtExcMsg(ex.stdout)}\nStderr:\n{fmtExcMsg(ex.stderr)}\n"


class NetworkChecker:
    def run_command(
        self, args: List[str], cwd: Path
    ) -> subprocess.CompletedProcess[str]:
        env = get_environment_suitable_for_subprocess()
        env["HGPLAIN"] = "1"
        return subprocess.run(
            args,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            check=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )

    def check_network(
        self,
        tracker: ProblemTracker,
        checkout_backing_repo: Path,
        checked_network_backing_repos: set[str],
        run_repo_checks: bool = True,
    ) -> None:
        if str(checkout_backing_repo) in checked_network_backing_repos:
            return
        checked_network_backing_repos.add(str(checkout_backing_repo))
        hg = os.environ.get("EDEN_HG_BINARY", "hg")
        try:
            self.run_command([hg, "debugnetworkdoctor"], checkout_backing_repo)
        except subprocess.CalledProcessError as ex:
            tracker.add_problem(
                ConnectivityProblem(
                    fmtProblemMessage(
                        "command 'hg debugnetworkdoctor' reported an error:", ex
                    )
                )
            )
            return
        except subprocess.TimeoutExpired as ex:
            tracker.add_problem(
                ConnectivityProblem(
                    fmtProblemMessage("command 'hg debugnetworkdoctor' timed out:", ex)
                )
            )
            return

        if run_repo_checks:
            try:
                self.run_command(
                    [hg, "debugnetwork", "--connection"], checkout_backing_repo
                )
            except subprocess.CalledProcessError as ex:
                # TODO: debugnetwork returns a variety of error numbers depending on the specific failure
                # but it should be covered by stdout. Noting in case we want to try to fix any of them
                # in the future.
                tracker.add_problem(
                    ConnectivityProblem(
                        fmtProblemMessage(
                            "command 'hg debugnetwork --connection' reported an error:",
                            ex,
                        )
                    )
                )
                return
            except subprocess.TimeoutExpired as ex:
                tracker.add_problem(
                    ConnectivityProblem(
                        fmtProblemMessage(
                            "command 'hg debugnetwork --connection' timed out:", ex
                        )
                    )
                )
                return

            try:
                speed_result = self.run_command(
                    [hg, "debugnetwork", "--speed", "--stable"], checkout_backing_repo
                )
            except subprocess.CalledProcessError as ex:
                # TODO: debugnetwork returns a variety of error numbers depending on the specific failure
                # but it should be covered by stdout. Noting in case we want to try to fix any of them
                # in the future.
                tracker.add_problem(NetworkSpeedProblem(fmtProblemMessage("", ex)))
                return
            except subprocess.TimeoutExpired as ex:
                tracker.add_problem(
                    ConnectivityProblem(
                        fmtProblemMessage(
                            f"command 'hg debugnetwork --speed' exceeded timeout of {NETWORK_TIMEOUT}s.\n"
                            "Your network might be too slow, please check the stdout for more details.\n"
                            f"There should be 2 rounds of download and upload speed tests.",
                            ex,
                        )
                    )
                )
                return

            # Latency + 4 speed entries, last entry is empty newline
            speed_values = speed_result.stdout.split("\n")[-6:-1]
            latency_str = re.search(
                r"Latency: (.*) \(average of (\d+) round-trips\)", speed_values[0]
            )
            if not latency_str:
                tracker.add_problem(
                    NetworkSpeedProblem("Could not get latency statistics")
                )
                return

            latency = parse_latency(latency_str.group(1))
            # 250ms
            if latency > 250:
                tracker.add_problem(NetworkLatencyProblem(latency))
                return

            speed_regex = r"Speed: \(round \d\) (uploaded|downloaded) (.*) MB in (.*) (s|ms|us) \((.*) Mbit/s, (.*) MiB/s\)"
            speed_outputs = []
            for entry in speed_values[1:5]:
                speed_str = re.search(speed_regex, entry)
                if not speed_str:
                    tracker.add_problem(
                        NetworkSpeedProblem("Could not get speed statistics")
                    )
                    return
                speed_outputs.append(float(speed_str.group(5)))

            # speed numbers taken from fixmywindows
            avg_download_speed = (speed_outputs[0] + speed_outputs[1]) / 2.0
            avg_upload_speed = (speed_outputs[2] + speed_outputs[3]) / 2.0
            if (
                avg_download_speed < MIN_DOWNLOAD_SPEED
                or avg_upload_speed < MIN_UPLOAD_SPEED
            ):
                tracker.add_problem(
                    NetworkSlowSpeedProblem([avg_download_speed, avg_upload_speed])
                )
                return
