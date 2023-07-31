/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DropdownField, DropdownFields} from '../DropdownFields';
import {T} from '../i18n';

import './DebugToolsMenu.css';

export default function DebugToolsMenu() {
  return (
    <DropdownFields
      title={<T>Internal Debugging Tools</T>}
      icon="pulse"
      data-testid="internal-debug-tools-dropdown"
      className="internal-debug-tools-dropdown">
      <DropdownField title={<T>Internal State</T>}>TODO</DropdownField>
    </DropdownFields>
  );
}
