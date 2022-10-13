import datetime
import os
import tempfile
from typing import Dict, NewType

import ghstack
import ghstack.logs

RawIndex = NewType('RawIndex', int)
FilteredIndex = NewType('FilteredIndex', int)


def get_argv(log_dir: str) -> str:
    argv = "Unknown"
    argv_fn = os.path.join(log_dir, 'argv')
    if os.path.exists(argv_fn):
        with open(argv_fn, 'r') as f:
            argv = f.read().rstrip()
    return argv


def get_status(log_dir: str) -> str:
    status = ""
    status_fn = os.path.join(log_dir, 'status')
    if os.path.exists(status_fn):
        with open(status_fn, 'r') as f:
            status = f.read().rstrip()
    return status


def main(latest: bool = False) -> None:

    log_base = ghstack.logs.base_dir()
    logs = os.listdir(log_base)
    logs.sort(reverse=True)

    filtered_mapping: Dict[FilteredIndex, RawIndex] = {}

    selected_index: FilteredIndex = FilteredIndex(0)
    next_index: FilteredIndex = FilteredIndex(0)
    if not latest:
        print("Which invocation would you like to report?")
        print()
        for (i, fn) in enumerate(logs):
            if next_index > 10:
                break

            raw_index = RawIndex(i)
            log_dir = os.path.join(log_base, fn)

            # Filter out rage
            # NB: This doesn't have to be 100% sound; just need to be
            # enough to good enough to filter out the majority of cases
            argv = get_argv(log_dir)
            argv_list = argv.split()

            if len(argv_list) >= 2 and argv_list[1] == "rage":
                continue

            if len(argv_list) >= 1:
                argv_list[0] = os.path.basename(argv_list[0])

            argv = ' '.join(argv_list)

            status = get_status(log_dir)
            if status:
                at_status = " at {}".format(status)
            else:
                at_status = ""

            cur_index = next_index
            next_index = FilteredIndex(next_index + 1)

            filtered_mapping[cur_index] = raw_index

            m = ghstack.logs.RE_LOG_DIRNAME.fullmatch(fn)
            if m:
                date = datetime.datetime.strptime(
                    m.group(1), ghstack.logs.DATETIME_FORMAT
                ).astimezone(tz=None).strftime("%a %b %d %H:%M:%S %Z")
            else:
                date = "Unknown"
            exception = "Succeeded"
            exception_fn = os.path.join(log_base, fn, 'exception')
            if os.path.exists(exception_fn):
                with open(exception_fn, 'r') as f:
                    exception = "Failed with: " + f.read().rstrip()

            print("{:<5}  {}  [{}]  {}{}"
                  .format("[{}].".format(cur_index), date, argv, exception, at_status))
        print()
        selected_index = FilteredIndex(
            int(input('(input individual number, for example 1 or 2)\n')))

    log_dir = os.path.join(log_base, logs[filtered_mapping[selected_index]])

    print()
    print("Writing report, please wait...")
    with tempfile.NamedTemporaryFile(mode='w', suffix=".log",
                                     prefix="ghstack", delete=False) as g:
        g.write("version: {}\n".format(ghstack.__version__))
        g.write("command: {}\n".format(get_argv(log_dir)))
        g.write("status: {}\n".format(get_status(log_dir)))
        g.write("\n")
        log_fn = os.path.join(log_dir, "ghstack.log")
        if os.path.exists(log_fn):
            with open(log_fn) as log:
                g.write(log.read())

    print("=> Report written to {}".format(g.name))
    print("Please include this log with your bug report!")
