/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable no-console */

import {randomId} from 'shared/utils';
import serverAPI from '../ClientToServerAPI';
import {joinPaths} from '../CwdSelector';
import type {CodeReviewIssue, CodeReviewResult} from './types';

const REQUEST_PREFIX = 'code-review-';

export async function runCodeReview(cwd: string): Promise<Map<string, Array<CodeReviewIssue>>> {
  if (!(await checkIfDevmateInstalled(cwd))) {
    throw new Error('Devmate is not installed.');
  }

  // Run code review
  const devmateCodeReviewRequestId = `${REQUEST_PREFIX}${randomId()}`;
  serverAPI.postMessage({
    type: 'runDevmateCommand',
    args: ['run', 'mcp_servers/code_review/review_code.md'],
    cwd,
    requestId: devmateCodeReviewRequestId,
  });
  const codeReviewResult = (
    await serverAPI.nextMessageMatching(
      'devmateCommandResult',
      message => message.result.requestId === devmateCodeReviewRequestId,
    )
  ).result;
  if (codeReviewResult.type === 'error') {
    // Devmate failed to run code review
    console.log('Devmate failed to run code review.');
    console.log(codeReviewResult.stderr);
    throw new Error('Devmate failed to run code review.');
  }

  console.log('Code review completed successfully!');
  // Parse the output of the code review command
  let parsedResults: Array<CodeReviewResult> = [];
  try {
    parsedResults = parseDevmateResponse(codeReviewResult.stdout);
    console.log('Code review result:', parsedResults);
  } catch (error) {
    console.error('Error parsing code review result:', error);
    throw new Error(`Error parsing code review result: ${error}`);
  }

  const fileToIssueMap = new Map<string, Array<CodeReviewIssue>>();
  for (const result of parsedResults) {
    for (const issue of result.codeIssues) {
      const filePath = joinPaths(cwd, issue.filepath);
      if (fileToIssueMap.has(filePath)) {
        fileToIssueMap.get(filePath)?.push({...issue, filepath: filePath});
      } else {
        fileToIssueMap.set(filePath, [{...issue, filepath: filePath}]);
      }
    }
  }

  return fileToIssueMap;
}

async function checkIfDevmateInstalled(cwd: string): Promise<boolean> {
  const devmateHelpRequestId = `${REQUEST_PREFIX}${randomId()}`;
  serverAPI.postMessage({
    type: 'runDevmateCommand',
    args: ['--help'],
    cwd,
    requestId: devmateHelpRequestId,
  });
  const devmateInstallationStatus = (
    await serverAPI.nextMessageMatching(
      'devmateCommandResult',
      message => message.result.requestId === devmateHelpRequestId,
    )
  ).result;
  if (devmateInstallationStatus.type === 'error') {
    // Devmate is not available, so we can't run code review
    console.log('Devmate is not available.');
    console.log(devmateInstallationStatus.stderr);
    return false;
  }
  return true;
}

function parseDevmateResponse(devmateResponse: string): Array<CodeReviewResult> {
  // Remove box-drawing characters and pipes, and normalize whitespace
  const cleanedResponse = devmateResponse
    .replace(/[\u2500-\u257F]/g, '') // Remove box drawing characters
    .replace(/^\s*\|\s?/gm, '') // Remove leading pipes and spaces
    .replace(/\r?\n/g, ' ') // Replace newlines with spaces
    .replace(/\s{2,}/g, ' '); // Collapse multiple spaces

  const matches = extractPotentialReviewers(cleanedResponse);
  const results = [];

  // Attempt to parse the JSON object from the first (only) match
  for (const match of matches) {
    try {
      const parsedJson = JSON.parse(match) as CodeReviewResult;
      results.push(parsedJson);
    } catch (error) {
      // Ignore invalid JSON objects
      continue;
    }
  }

  return results;
}

/* Regex is not great for parsing JSON, so instead let's try to extract the objects
manually using a naive bracket-matching approach. */
function extractPotentialReviewers(text: string): Array<string> {
  // Narrow the search to the observations for the format_code_review_tool
  const observations = extractBetweenAfter(
    text,
    'Calling tool: format_code_reviews_tool',
    'Devmate (Observing)',
    'Devmate (Thinking)',
  );
  if (observations == null) {
    throw new Error('Could not find observations for format_code_review_tool');
  }
  // Find all occurrences of "Review:" followed by a JSON object
  const results = [];
  let idx = 0;
  while ((idx = observations.indexOf('Review:', idx)) !== -1) {
    const start = observations.indexOf('{', idx);
    if (start === -1) {
      break;
    }
    let end = start,
      depth = 0;
    do {
      if (observations[end] === '{') {
        depth++;
      }
      if (observations[end] === '}') {
        depth--;
      }
      end++;
    } while (depth > 0 && end < observations.length);
    const jsonStr = observations.slice(start, end);
    results.push(jsonStr);
    idx = end;
  }
  return results;
}

function extractBetweenAfter(
  text: string,
  afterStr: string,
  startStr: string,
  endStr: string,
): string | null {
  // Find the position after the 'afterStr'
  const afterIndex = text.indexOf(afterStr);
  if (afterIndex === -1) {
    return null;
  }
  // Start searching for startStr after afterStr
  const startIndex = text.indexOf(startStr, afterIndex + afterStr.length);
  if (startIndex === -1) {
    return null;
  }
  // Start searching for endStr after startStr
  const endIndex = text.indexOf(endStr, startIndex + startStr.length);
  if (endIndex === -1) {
    return null;
  }
  // Extract and return the substring between startStr and endStr
  return text.substring(startIndex + startStr.length, endIndex);
}
