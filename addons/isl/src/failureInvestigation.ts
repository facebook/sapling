/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Evidence about a failure, never a command to replay. */
export type FailedOperationContext = {
  operationId: string;
  operationName: string;
  exitCode: number;
  output: string;
  /** Reported by the producer; omitted content cannot be inferred from the retained tail. */
  outputTruncated: boolean;
};

const MAX_OUTPUT_BYTES = 16 * 1024;
const MAX_OUTPUT_LINES = 100;

export function createFailedOperationContext(
  operationId: string,
  operationName: string,
  exitCode: number,
  outputLines: readonly string[],
): FailedOperationContext {
  const lines = outputLines.join('\n').split('\n');
  const bytes = new TextEncoder().encode(lines.slice(-MAX_OUTPUT_LINES).join('\n'));
  let start = Math.max(0, bytes.length - MAX_OUTPUT_BYTES);
  // Keep a complete UTF-8 code point at the beginning of the retained tail.
  while (start < bytes.length && bytes[start] >= 0x80 && bytes[start] < 0xc0) {
    start++;
  }
  return {
    operationId,
    operationName,
    exitCode,
    output: new TextDecoder().decode(bytes.subarray(start)),
    outputTruncated: lines.length > MAX_OUTPUT_LINES || start > 0,
  };
}

/** Copy only the bounded evidence fields received across the webview boundary. */
export function parseFailedOperationContext(value: unknown): FailedOperationContext {
  if (typeof value !== 'object' || value == null) {
    throw new Error('Missing failure context');
  }
  const context = value as Partial<FailedOperationContext>;
  if (
    typeof context.operationId !== 'string' ||
    context.operationId.length === 0 ||
    context.operationId.length > 128 ||
    typeof context.operationName !== 'string' ||
    context.operationName.length === 0 ||
    context.operationName.length > 128 ||
    typeof context.exitCode !== 'number' ||
    !Number.isSafeInteger(context.exitCode) ||
    context.exitCode === 0 ||
    typeof context.output !== 'string' ||
    new TextEncoder().encode(context.output).length > MAX_OUTPUT_BYTES ||
    context.output.split('\n').length > MAX_OUTPUT_LINES ||
    typeof context.outputTruncated !== 'boolean'
  ) {
    throw new Error('Invalid failure context');
  }
  return {
    operationId: context.operationId,
    operationName: context.operationName,
    exitCode: context.exitCode,
    output: context.output,
    outputTruncated: context.outputTruncated,
  };
}
