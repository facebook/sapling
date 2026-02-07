/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UserFragment} from './generated/graphql';
import type {ChangeEvent} from 'react';

import {gitHubRepoAssignableUsers, gitHubRepoAssignableUsersQuery} from './jotai';
import useDebounced from './useDebounced';
import {ActionList, Avatar, Box, Spinner, TextInput} from '@primer/react';
import {useAtom, useAtomValue} from 'jotai';
import React, {Suspense, useCallback, useEffect, useState} from 'react';

type Props = {
  existingUserIDs: ReadonlySet<string>;
  onSelect: (user: UserFragment, isExisting: boolean) => void;
};

function LoadingFallback(): React.ReactElement {
  return (
    <Box display="flex" justifyContent="center" padding={3}>
      <Spinner size="medium" />
    </Box>
  );
}

function RepoAssignableUsersInputInner({
  existingUserIDs,
  onSelect,
}: Props): React.ReactElement {
  const users = useAtomValue(gitHubRepoAssignableUsers);
  const [query, setQuery] = useAtom(gitHubRepoAssignableUsersQuery);
  const [queryInput, setQueryInput] = useState(query);
  const setQueryAtom = useDebounced(setQuery);
  const resetQueryAtom = useCallback(() => {
    setQuery('');
  }, [setQuery]);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const {value} = e.currentTarget;
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
        <TextInput value={queryInput} onChange={onChange} placeholder="Search users" />
      </Box>
      <ActionList.Divider />
      {users.map(user => (
        <ActionList.Item
          key={user.id}
          selected={existingUserIDs.has(user.id)}
          onSelect={() => onSelect(user, existingUserIDs.has(user.id))}>
          <ActionList.LeadingVisual>
            <Avatar src={user.avatarUrl} />
          </ActionList.LeadingVisual>
          {user.login}
        </ActionList.Item>
      ))}
    </ActionList>
  );
}

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function RepoAssignableUsersInput(props: Props): React.ReactElement {
  return (
    <Suspense fallback={<LoadingFallback />}>
      <RepoAssignableUsersInputInner {...props} />
    </Suspense>
  );
});
