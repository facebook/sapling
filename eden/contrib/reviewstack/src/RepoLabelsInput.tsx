/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LabelFragment} from './generated/graphql';
import type {ChangeEvent} from 'react';

import CenteredSpinner from './CenteredSpinner';
import {gitHubRepoLabels, gitHubRepoLabelsQuery} from './jotai';
import useDebounced from './useDebounced';
import {ActionList, Box, TextInput} from '@primer/react';
import {useAtom, useAtomValue} from 'jotai';
import React, {Suspense, useCallback, useEffect, useState} from 'react';

type Props = {
  existingLabelIDs: Set<string>;
  onSelect: (label: LabelFragment, isExisting: boolean) => void;
};

function RepoLabelsInputInner({
  existingLabelIDs,
  onSelect,
}: Props): React.ReactElement {
  const repoLabels = useAtomValue(gitHubRepoLabels);
  const [query, setQuery] = useAtom(gitHubRepoLabelsQuery);
  const [queryInput, setQueryInput] = useState(query);
  const setQueryAtom = useDebounced(setQuery);
  const resetQueryAtom = useCallback(() => {
    setQuery('');
  }, [setQuery]);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const value = e.currentTarget.value;
      setQueryAtom(value);
      setQueryInput(value);
    },
    [setQueryAtom, setQueryInput],
  );

  useEffect(() => {
    return () => {
      resetQueryAtom();
    };
  }, [resetQueryAtom]);

  return (
    <ActionList selectionVariant="single">
      <Box display="flex" flexDirection="column" alignItems="stretch" padding={1}>
        <TextInput value={queryInput} onChange={onChange} placeholder="Search labels" />
      </Box>
      <ActionList.Divider />
      {repoLabels.map(({id, name, color}) => (
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
}

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function RepoLabelsInput(props: Props): React.ReactElement {
  return (
    <Suspense fallback={<CenteredSpinner message="Loading labels..." />}>
      <RepoLabelsInputInner {...props} />
    </Suspense>
  );
});

function LabelColorCircle({color}: {color: string}): React.ReactElement {
  return <Box bg={`#${color}`} width={12} height={12} borderRadius={10} />;
}
