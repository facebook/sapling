#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
from enum import IntEnum
from typing import Any, List, Optional, Set

from eden.fs.cli import ui
from eden.fs.cli.config import AbstractEdenInstance, configutil

from .util import format_exception


class RemediationError(Exception):
    pass


class ProblemSeverity(IntEnum):
    # Note that we intentionally want to be able to compare severity values
    # using < and > operators.
    ALL = 0
    ADVICE = 3
    POTENTIALLY_SERIOUS = 4  # A bit more serious than ADVICE (for filtering)
    ERROR = 10
    MELTDOWN = 255  # A condition in which nothing is safe; in practice, used to filter messages, but not actually used (for now) for any known problem


class ProblemBase(abc.ABC):
    @abc.abstractmethod
    def description(self) -> str:
        "Return the description of this problem."

    def format_description(self, *, debug: bool = False) -> str:
        return self.description()

    def severity(self) -> ProblemSeverity:
        """Return the problem severity.

        Defaults to ERROR if not overridden.
        """
        return ProblemSeverity.ERROR

    def get_manual_remediation_message(self) -> Optional[str]:
        "Return a message explaining how to manually fix this problem."
        return None


class FixableProblem(ProblemBase):
    @abc.abstractmethod
    def dry_run_msg(self) -> str:
        """Return a string to print for dry-run operations."""

    @abc.abstractmethod
    def start_msg(self) -> str:
        """Return a string to print when starting the remediation."""

    @abc.abstractmethod
    def perform_fix(self) -> None:
        """Attempt to automatically fix the problem."""


class Problem(ProblemBase):
    def __init__(
        self,
        description: str,
        remediation: Optional[str] = None,
        *,
        exception: Optional[Exception] = None,
        severity: ProblemSeverity = ProblemSeverity.ERROR,
    ) -> None:
        self._description = description
        self._remediation = remediation
        self._exception = exception
        self._severity = severity

    def description(self) -> str:
        return self._description

    def format_description(self, *, debug: bool = False) -> str:
        description = self.description()
        exception = getattr(self, "_exception", None)
        if not exception:
            return description
        return f"{description}\n{format_exception(exception, debug)}"

    def severity(self) -> ProblemSeverity:
        return self._severity

    def get_manual_remediation_message(self) -> Optional[str]:
        return self._remediation


class UnexpectedCheckError(Problem):
    """A helper class for reporting about unexpected exceptions that occur
    when running checks."""

    def __init__(self) -> None:
        import traceback

        tb = traceback.format_exc()
        description = f"unexpected error while checking for problems: {tb}"
        super().__init__(description)


class ProblemTracker(abc.ABC):
    # using_edenfs will be set to False if EdenFS is not running and there
    # are no configured EdenFS checkouts.
    using_edenfs: bool = True

    def __init__(self, instance: AbstractEdenInstance) -> None:
        self.ignored_problem_types: Set[str] = set()
        self._instance = instance
        self._ignored_problem_class_names: configutil.Strs = instance.get_config_strs(
            "doctor.ignored-problem-class-names", default=configutil.Strs()
        )

    def add_problem(self, problem: ProblemBase) -> None:
        """Record a new problem"""

        problem_type = type(problem).__name__
        if problem_type in self._ignored_problem_class_names:
            self.ignored_problem_types.add(problem_type)
        else:
            self.add_problem_impl(problem)

    def add_problem_impl(self, problem: ProblemBase) -> None: ...


class OutWriterWithSeverityFilter:
    def __init__(self, out: ui.Output, min_severity_to_report: int) -> None:
        self.out = out
        self.min_severity_to_report = min_severity_to_report

    def write(self, problem_severity: int, *args: Any, **kwargs: Any) -> None:
        if problem_severity >= self.min_severity_to_report:
            self.out.write(*args, **kwargs)

    def writeln(self, problem_severity: int, *args: Any, **kwargs: Any) -> None:
        if problem_severity >= self.min_severity_to_report:
            self.out.writeln(*args, **kwargs)


class ProblemFixer(ProblemTracker):
    def __init__(
        self,
        instance: AbstractEdenInstance,
        out: ui.Output,
        debug: bool = False,
        min_severity_to_report: ProblemSeverity = ProblemSeverity.ALL,
    ) -> None:
        super().__init__(instance)
        self._filtered_out = OutWriterWithSeverityFilter(out, min_severity_to_report)
        self.debug = debug
        self.num_problems = 0
        self.num_fixed_problems = 0
        self.num_failed_fixes = 0
        self.num_manual_fixes = 0
        self.problem_types: Set[str] = set()
        self.problem_description: List[str] = []

    def add_problem_impl(self, problem: ProblemBase) -> None:
        self.num_problems += 1
        problem_class = problem.__class__.__name__
        self.problem_types.add(problem_class)
        self._filtered_out.writeln(
            problem.severity(), "- Found problem:", fg=self._filtered_out.out.YELLOW
        )
        description = problem.format_description(debug=self.debug)
        self.problem_description.append(description)
        self._filtered_out.writeln(problem.severity(), description)
        if isinstance(problem, FixableProblem):
            self.fix_problem(problem)
        else:
            self.num_manual_fixes += 1
            msg = problem.get_manual_remediation_message()
            if msg:
                self._filtered_out.write(problem.severity(), msg, end="\n\n")

    def fix_problem(self, problem: FixableProblem) -> None:
        self._filtered_out.write(
            problem.severity(), f"{problem.start_msg()}...", flush=True
        )
        try:
            problem.perform_fix()
            self._filtered_out.write(
                problem.severity(),
                "fixed",
                fg=self._filtered_out.out.GREEN,
                end="\n\n",
                flush=True,
            )
            self.num_fixed_problems += 1
        except Exception as ex:
            self._filtered_out.writeln(
                problem.severity(), "error", fg=self._filtered_out.out.RED
            )
            self._filtered_out.write(
                problem.severity(),
                f"Failed to fix problem: {format_exception(ex, with_tb=True)}",
                end="\n\n",
                flush=True,
            )
            self.num_failed_fixes += 1


class DryRunFixer(ProblemFixer):
    def fix_problem(self, problem: FixableProblem) -> None:
        self._filtered_out.write(problem.severity(), problem.dry_run_msg(), end="\n\n")
