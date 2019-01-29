#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import collections
import configparser
from typing import (
    Any,
    DefaultDict,
    Dict,
    List,
    Mapping,
    MutableMapping,
    Optional,
    Tuple,
)

from .configinterpolator import EdenConfigInterpolator


# Convert the passed EdenConfigParser to a raw dictionary (without interpolation)
# Useful for updating configuration files in different formats.
def config_to_raw_dict(config: "EdenConfigParser") -> collections.OrderedDict:
    rslt = collections.OrderedDict()  # type: collections.OrderedDict
    for section in config.sections():
        rslt[section] = collections.OrderedDict()
        for k, v in config.items(section, raw=True):
            rslt[section][k] = v
    return rslt


ConfigValue = str
ConfigSectionName = str
ConfigOptionName = str


class EdenConfigParser:
    _interpolator: configparser.Interpolation
    _sections: DefaultDict[ConfigSectionName, Dict[ConfigOptionName, ConfigValue]]

    def __init__(self, interpolation: Optional[EdenConfigInterpolator] = None) -> None:
        super().__init__()
        self._interpolator = (
            configparser.Interpolation() if interpolation is None else interpolation
        )
        self._sections = collections.defaultdict(dict)

    def read_dict(
        self, dictionary: Mapping[ConfigSectionName, Mapping[ConfigOptionName, Any]]
    ) -> None:
        for section, options in dictionary.items():
            for option, value in options.items():
                self._sections[section][option] = self._make_storable_value(value)

    def sections(self) -> List[ConfigSectionName]:
        return list(self._sections.keys())

    def items(
        self, section: ConfigSectionName, raw: bool = False
    ) -> List[Tuple[ConfigOptionName, ConfigValue]]:
        options = self._sections.get(section)
        if options is None:
            raise configparser.NoSectionError(section)
        return [
            (
                option,
                self._interpolate_value_if_necessary(
                    raw=raw, section=section, option=option, value=value
                ),
            )
            for option, value in options.items()
        ]

    def get(
        self,
        section: ConfigSectionName,
        option: ConfigOptionName,
        fallback: ConfigValue,
    ) -> ConfigValue:
        options = self._sections.get(section)
        if options is None:
            return fallback
        if option not in options:
            return fallback
        value = options[option]
        return self._interpolate_value_if_necessary(
            raw=False, section=section, option=option, value=value
        )

    def has_section(self, section: ConfigSectionName) -> bool:
        return section in self._sections

    def __setitem__(
        self,
        section: ConfigSectionName,
        options: Mapping[ConfigOptionName, ConfigValue],
    ) -> None:
        self._sections[section] = {
            option: self._make_storable_value(value)
            for option, value in options.items()
        }

    @property
    def _defaults(self) -> Mapping[ConfigOptionName, ConfigValue]:
        return {}

    @property
    def _parser(
        self
    ) -> MutableMapping[ConfigSectionName, Mapping[ConfigOptionName, ConfigValue]]:
        return {}

    def _interpolate_value_if_necessary(
        self,
        raw: bool,
        section: ConfigSectionName,
        option: ConfigOptionName,
        value: ConfigValue,
    ) -> ConfigValue:
        if not raw:
            value = self._interpolator.before_get(
                self._parser, section, option, value, self._defaults
            )
        return value

    def _make_storable_value(self, value: Any) -> ConfigValue:
        # TODO(strager): Avoid converting values to strings.
        return str(value)
