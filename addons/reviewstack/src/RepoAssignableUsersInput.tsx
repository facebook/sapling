/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UserFragment} from './generated/graphql';
import type {ChangeEvent} from 'react';

import {gitHubRepoAssignableUsers, gitHubRepoAssignableUsersQuery} from './recoil';
import useDebounced from './useDebounced';
import {ActionList, Avatar, Box, TextInput} from '@primer/react';
import React, {useCallback, useEffect, useState} from 'react';
import {useRecoilState, useRecoilValueLoadable, useResetRecoilState} from 'recoil';

type Props = {
  existingUserIDs: ReadonlySet<string>;
  onSelect: (user: UserFragment, isExisting: boolean) => void;
};

// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function RepoAssignableUsersInput({
  existingUserIDs,
  onSelect,
}: Props): React.ReactElement {
  const users = useRecoilValueLoadable(gitHubRepoAssignableUsers).valueMaybe();
  const [query, setQuery] = useRecoilState(gitHubRepoAssignableUsersQuery);
  const [queryInput, setQueryInput] = useState(query);
  const setQueryAtom = useDebounced(setQuery);
  const resetQueryAtom = useResetRecoilState(gitHubRepoAssignableUsersQuery);

  const onChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const {value} = e.currentTarget;
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
          loading={users == null}
          placeholder="Search users"
        />
      </Box>
      <ActionList.Divider />
      {(users ?? []).map(user => (
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
});
