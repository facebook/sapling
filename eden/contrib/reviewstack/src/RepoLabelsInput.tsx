/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LabelFragment} from './generated/graphql';
import type {ChangeEvent} from 'react';

import {gitHubRepoLabels, gitHubRepoLabelsQuery} from './recoil';
import useDebounced from './useDebounced';
import {ActionList, Box, TextInput} from '@primer/react';
import React, {useCallback, useEffect, useState} from 'react';
import {useRecoilState, useRecoilValueLoadable, useResetRecoilState} from 'recoil';

type Props = {
  existingLabelIDs: Set<string>;
  onSelect: (label: LabelFragment, isExisting: boolean) => void;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function RepoLabelsInput({
  existingLabelIDs,
  onSelect,
}: Props): React.ReactElement {
  const repoLabels = useRecoilValueLoadable(gitHubRepoLabels).valueMaybe();
  const [query, setQuery] = useRecoilState(gitHubRepoLabelsQuery);
  const [queryInput, setQueryInput] = useState(query);
  const setQueryAtom = useDebounced(setQuery);
  const resetQueryAtom = useResetRecoilState(gitHubRepoLabelsQuery);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const value = e.currentTarget.value;
      setQueryAtom(value);
      setQueryInput(value);
    },
    [setQueryAtom, setQueryInput],
  );

  useEffect(() => resetQueryAtom, [resetQueryAtom]);

  return (
    <ActionList selectionVariant="single">
      <Box display="flex" flexDirection="column" alignItems="stretch" padding={1}>
        <TextInput
          value={queryInput}
          onChange={onChange}
          loading={repoLabels == null}
          placeholder="Search labels"
        />
      </Box>
      <ActionList.Divider />
      {(repoLabels ?? []).map(({id, name, color}) => (
        <ActionList.Item
          key={id}
          selected={existingLabelIDs.has(id)}
          onSelect={() => onSelect({id, name, color}, existingLabelIDs.has(id))}>
          <ActionList.LeadingVisual>
            <LabelColorCircle color={color} />
          </ActionList.LeadingVisual>
          {name}
        </ActionList.Item>
      ))}
    </ActionList>
  );
});

function LabelColorCircle({color}: {color: string}): React.ReactElement {
  return <Box bg={`#${color}`} width={12} height={12} borderRadius={10} />;
}
