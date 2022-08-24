# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Dict

from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker

try:
    from .facebook.internal_error_messages import (
        get_elevated_recent_writes_error_message_link,
    )
except ImportError:

    def get_elevated_recent_writes_error_message_link() -> str:
        return "eden redirect --help"


def check_recent_writes(
    tracker: ProblemTracker,
    instance: EdenInstance,
    debug: bool,
) -> None:

    # setting true will output a top ten list of largest write operations.
    verboseOutput = debug

    # TODO: this only checks for Windows, we should do the same for Linux/macOS.
    COUNTER_REGEX = r"(prjfs\.((fileHandleClosedFileDeleted)|(fileHandleClosedFileModified)|(fileOverwritten)|(fileRenamed)|(newFileCreated)).*count\.(600))"

    # Don't report counters that has a count less than this number
    minWriteThresholdString = instance.get_config_value(
        "doctor.recent-writes-problem-threshold", "10000"
    )

    # this can throw a parse exception, but instead of handling it here I think it's better to
    # allow this to fall through to the try that wraps the checker for consistency.
    minWriteThreshold = int(minWriteThresholdString)

    if debug:
        minWriteThreshold = -1

    # Don't show more than this many results, even if they exceed the minWriteThreshold
    # show the highest number of counts first
    maxNumberDisplayed = 10

    # Create a thrift client and call the getCounter method
    with instance.get_thrift_client_legacy() as client:
        result = client.getRegexCounters(COUNTER_REGEX)

    reportError = should_we_report_error(result, minWriteThreshold)

    if reportError:
        # Purty it up

        message = format_output_message(result, minWriteThreshold)

        if verboseOutput:
            verboseMessage = format_output_message_verbose(
                result, maxNumberDisplayed, minWriteThreshold
            )
            message += verboseMessage

        # Add to the tracker as some good[citation needed] advice.
        tracker.add_problem(
            ElevatedRecentWritesProblem(
                description=message,
                severity=ProblemSeverity.ADVICE,
            )
        )


def should_we_report_error(results: Dict[str, int], minWriteThreshold: int) -> bool:

    total = 0
    for val in results:
        total += results[val]

    return total > minWriteThreshold


def format_output_message_verbose(
    result: Dict[str, int], maxNumberDisplayed: int, minWriteThreshold: int
) -> str:

    message = "\nCount List:"

    warnlist = []
    for val in result:
        if result[val] > minWriteThreshold:
            warnlist.append(WriteCounter(val.replace("prjfs.", ""), result[val]))

    warnlist.sort(key=lambda x: x.count, reverse=True)

    maxNameSize = 0
    cnt = 0
    for warn in warnlist:
        namesize = len(warn.name)
        if namesize > maxNameSize:
            maxNameSize = namesize
        cnt += 1
        if cnt > maxNumberDisplayed:
            break

    maxNameSize += 2
    cnt = 0
    for warn in warnlist:
        namesize = len(warn.name)
        whitespace = ""
        for _i in range(namesize, maxNameSize):
            whitespace += " "

        message += f"\n{warn.name}{whitespace}{warn.count}"
        cnt += 1
        if cnt > maxNumberDisplayed:
            break

    return message


def format_output_message(result: Dict[str, int], minWriteThreshold: int) -> str:

    totalCounts = 0
    for val in result:
        totalCounts += result[val]

    message = (
        f"We have detected {totalCounts} write operations to the virtual repo.\n"
        "These are expensive operations and you may be able to increaes your performance by using a redirect\n"
        "for non-source controlled items such as build products or temporary files:\n"
        f"See: {get_elevated_recent_writes_error_message_link()}"
    )

    return message


class WriteCounter:
    __slots__ = ("name", "count")

    def __init__(self, name: str, count: int) -> None:
        self.name = name
        self.count = count


class ElevatedRecentWritesProblem(Problem):
    pass
