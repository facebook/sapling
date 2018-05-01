#!/usr/bin/env bash

HG=${HG:-"hg"}
$HG --config extensions.dump_commands=hg_dump_commands_ext.py dump_commands
rustfmt hg_python_commands.rs
