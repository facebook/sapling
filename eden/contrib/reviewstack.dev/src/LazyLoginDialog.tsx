/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 * Licensed under the MIT license in the upstream LICENSE file.
 */
import type {CustomLoginDialogProps} from 'reviewstack/src/LoginDialog';

import DefaultLoginDialog from './DefaultLoginDialog';

export default function LazyLoginDialog(props: CustomLoginDialogProps) {
  return <DefaultLoginDialog {...props} />;
}
