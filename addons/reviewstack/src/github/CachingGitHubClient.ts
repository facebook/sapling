/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  AddCommentMutationData,
  AddLabelsToLabelableInput,
  AddLabelsToLabelableMutationData,
  AddPullRequestReviewInput,
  AddPullRequestReviewMutationData,
  AddPullRequestReviewCommentInput,
  AddPullRequestReviewCommentMutationData,
  LabelFragment,
  PullRequestReviewDecision,
  PullRequestState,
  RemoveLabelsFromLabelableInput,
  RemoveLabelsFromLabelableMutationData,
  RequestReviewsInput,
  RequestReviewsMutationData,
  StackPullRequestFragment,
  SubmitPullRequestReviewInput,
  SubmitPullRequestReviewMutationData,
  UserFragment,
} from '../generated/graphql';
import type GitHubClient from './GitHubClient';
import type {PullRequest} from './pullRequestTimelineTypes';
import type {PullsQueryInput, PullsWithPageInfo} from './pullsTypes';
import type {CommitComparison} from './restApiTypes';
import type {Blob, Commit, GitObject, GitObjectID, ID, Tree} from './types';

import {globalCacheStats} from './GitHubClientStats';
import {subscribeToLogout} from './logoutBroadcastChannel';
import rejectAfterTimeout from 'shared/rejectAfterTimeout';

const DB_VERSION = 2;
const DB_NAME = `github-objects-v${DB_VERSION}`;
const DB_COMMIT_STORE_NAME = 'commit';
const DB_TREE_STORE_NAME = 'tree';
const DB_BLOB_STORE_NAME = 'blob';
const PR_FRAGMENT_STORE_NAME = 'pr-fragment';

interface NormalizedCommit extends GitObject {
  oid: GitObjectID;
  id: ID;
  url: string;
  message: string;
  messageHeadline: string;
  messageHeadlineHTML: string;
  messageBody: string;
  messageBodyHTML: string;
  parents: GitObjectID[];
  tree_oid: GitObjectID;
  committedDate: string;
}

type NormalizedStackPullRequestFragment = {
  owner: string;
  name: string;
  number: number;
  title: string;
  updatedAt: string;
  state: PullRequestState;
  reviewDecision: PullRequestReviewDecision | null | undefined;
  headRefOid: GitObjectID;
  numComments: number;
};

/** Name of an IDBObjectStore in our IDBDatabase. */
type Store =
  | typeof DB_COMMIT_STORE_NAME
  | typeof DB_TREE_STORE_NAME
  | typeof DB_BLOB_STORE_NAME
  | typeof PR_FRAGMENT_STORE_NAME;

type StoreTypes = {
  [DB_COMMIT_STORE_NAME]: NormalizedCommit;
  [DB_TREE_STORE_NAME]: Tree;
  [DB_BLOB_STORE_NAME]: Blob;
  [PR_FRAGMENT_STORE_NAME]: NormalizedStackPullRequestFragment;
};

/**
 * Represents an open readwrite transaction for the specified store.
 * Callers are expected to invoke add() as many times as necessary
 * (awaiting the result if they want confirmation that the IDBRequest for the
 * corresponding add() call succeeded) and finally invoking commit() when
 * finished.
 */
class OpenTransaction<S extends Store, O = StoreTypes[S]> {
  private tx: IDBTransaction;
  private store: IDBObjectStore;
  private txResult: Promise<void>;

  constructor(db: IDBDatabase, private storeName: S) {
    this.tx = db.transaction(storeName, 'readwrite');
    this.store = this.tx.objectStore(storeName);
    this.txResult = new Promise((resolve, reject) => {
      this.tx.oncomplete = () => resolve();
      this.tx.onerror = event => {
        if ((event?.target as IDBRequest).error?.name === 'ConstraintError') {
          switch (this.storeName) {
            case DB_BLOB_STORE_NAME: {
              ++globalCacheStats.duplicateKeyBlob;
              break;
            }
            case DB_TREE_STORE_NAME: {
              ++globalCacheStats.duplicateKeyTree;
              break;
            }
            case DB_COMMIT_STORE_NAME: {
              ++globalCacheStats.duplicateKeyCommit;
              break;
            }
          }
          resolve();
        } else {
          reject();
        }
      };
    });
  }

  add(obj: O): Promise<void> {
    return new Promise((resolve, reject) => {
      let request;
      try {
        request = this.store.add(obj);
      } catch (e) {
        return reject(e);
      }

      request.onsuccess = _event => resolve();
      request.onerror = event => {
        if ((event?.target as IDBRequest).error?.name === 'ConstraintError') {
          let identifier = 'unknown';
          if (implementsGitObject(obj)) {
            identifier = obj.oid;
          } else {
            const pr = obj as unknown as NormalizedStackPullRequestFragment;
            identifier = `${pr.owner}/${pr.name}/${pr.number}`;
          }
          // eslint-disable-next-line no-console
          console.info(`${identifier} already added to store ${this.store.name}`);
          resolve();
        } else {
          reject(event);
        }
      };
    });
  }

  /**
   * Returns a Promise that resolves when the underlying IDBTransaction
   * completes.
   */
  commit(): Promise<void> {
    this.tx.commit();
    return this.txResult;
  }
}

function implementsGitObject(obj: unknown): obj is GitObject {
  return typeof obj === 'object' && obj !== null && 'oid' in obj;
}

/**
 * Decorates a GitHubClient, but uses IndexedDB as a caching layer.
 */
export default class CachingGitHubClient implements GitHubClient {
  /**
   * owner and name must be non-null if the getStackPullRequests() will be
   * used.
   */
  constructor(
    private db: IDBDatabase,
    private client: GitHubClient,
    private owner: string | null,
    private name: string | null,
  ) {}

  async getCommit(oid: GitObjectID): Promise<Commit | null> {
    const cachedCommit = await this.getCachedCommit(oid);
    if (cachedCommit != null) {
      ++globalCacheStats.cacheCommitReads;
      return cachedCommit;
    }

    const commit = await this.client.getCommit(oid);
    if (commit != null) {
      // Note that multiple commits may have the same root Tree object (this is
      // particularly common with stacks created via ghstack), in which case the
      // underlying IDBTransaction will fail with a ConstraintError if the Tree
      // already exists in IndexedDB as a result of persisting some other
      // Commit. (Note that OpenTransaction will swallow this error so the
      // Promise returned by add() will not reject.)
      //
      // There is no great way to avoid this without incurring the cost of an
      // extra read, which hardly seems worth the cost and is not 100% reliable
      // because it is subject to TOCTOU races. For the curious,
      // GitHubClientStats is available to see how often this happens, in
      // practice.
      //
      // Further, we choose not to await the calls to tx.add() (or even invoke
      // tx.commit()) and return to the caller before the Tree or Commit is
      // persisted to IndexedDB. This knowingly runs the risk of fetching the
      // Tree or Commit multiple times.
      {
        const tx = new OpenTransaction(this.db, DB_TREE_STORE_NAME);
        tx.add(commit.tree);
      }
      {
        const tx = new OpenTransaction(this.db, DB_COMMIT_STORE_NAME);
        const normalizedCommit = normalizeCommit(commit);
        tx.add(normalizedCommit);
      }
    }
    return commit;
  }

  getCommitComparison(base: GitObjectID, head: GitObjectID): Promise<CommitComparison | null> {
    // No caching done for now.
    return this.client.getCommitComparison(base, head);
  }

  async getTree(oid: GitObjectID): Promise<Tree | null> {
    const cachedTree = await this.getCachedTree(oid);
    if (cachedTree != null) {
      ++globalCacheStats.cacheTreeReads;
      return cachedTree;
    }

    const tree = await this.client.getTree(oid);
    // Note that if tree is null, it is possible that a Tree with the
    // GitObjectID comes into existence later, so we should not write a null
    // entry to IndexedDB.
    if (tree != null) {
      // We choose not to await the call to tx.add() (or even invoke
      // tx.commit()) and return to the caller before the Tree is persisted to
      // IndexedDB. This knowingly runs the risk of fetching the Tree multiple
      // times.
      const tx = new OpenTransaction(this.db, DB_TREE_STORE_NAME);
      tx.add(tree);
    }
    return tree;
  }

  async getBlob(oid: GitObjectID): Promise<Blob | null> {
    const cachedBlob = await this.getCachedBlob(oid);
    if (cachedBlob != null) {
      ++globalCacheStats.cacheBlobReads;
      return cachedBlob;
    }

    const blob = await this.client.getBlob(oid);
    // Note that if blob is null, it is possible that a Blob with the
    // GitObjectID comes into existence later, so we should not write a null
    // entry to IndexedDB.
    if (blob != null) {
      // It is imperative that the blob be persisted to IndexedDB before
      // returning because diffServiceWorker assumes that if it receives a
      // GitObjectID for a Blob, it will be able to read it from IndexedDB.
      const tx = new OpenTransaction(this.db, DB_BLOB_STORE_NAME);
      await tx.add(blob);
      await tx.commit();
    }
    return blob;
  }

  getPullRequest(pr: number): Promise<PullRequest | null> {
    // No caching done because the PR could have been updated since the PR data
    // were requested last.
    return this.client.getPullRequest(pr);
  }

  getPullRequests(input: PullsQueryInput): Promise<PullsWithPageInfo | null> {
    return this.client.getPullRequests(input);
  }

  getRepoAssignableUsers(query: string | null): Promise<UserFragment[]> {
    return this.client.getRepoAssignableUsers(query);
  }

  getRepoLabels(query: string | null): Promise<LabelFragment[]> {
    return this.client.getRepoLabels(query);
  }

  // TODO: It should be possible to invalidate these entries because they can
  // get out of date.
  async getStackPullRequests(prs: number[]): Promise<StackPullRequestFragment[]> {
    // First, we try to read as many fragments from the cache as possible.
    const cachedFragments = await this.getCachedPRFragments(prs);

    // We record each cache miss with the necessary bookkeeping information to
    // patch up the cachedFragments array.
    const prsToFetch: number[] = [];
    const prsToFetchIndex: number[] = [];
    cachedFragments.forEach((fragment, index) => {
      if (fragment == null) {
        prsToFetch.push(prs[index]);
        prsToFetchIndex.push(index);
      }
    });

    // After fetching the cache misses, we write them back into the original
    // cachedFragments array as well as IndexedDB.
    const fetchedFragments = await this.client.getStackPullRequests(prsToFetch);
    const {owner, name} = this.getOwnerAndName();

    const tx = new OpenTransaction(this.db, PR_FRAGMENT_STORE_NAME);
    await Promise.all(
      fetchedFragments.map((fragment, index) => {
        const originalIndex = prsToFetchIndex[index];
        cachedFragments[originalIndex] = fragment;
        const normalizedFragment = normalizePullRequestFragment(owner, name, fragment);
        // Stores a StackPullRequestFragment in IndexedDB, which uses
        // [owner, name, number] as the key. Of note:
        // - Unlike blobs and trees where the key is a content hash, the value of a
        //   StackPullRequestFragment associated with the key can change over time
        //   because it includes fields like title, updatedAt, etc. It needs to be
        //   possible to evict/update entries in the table, as appropriate.
        // - StackPullRequestFragment is defined in StackPullRequestFragment.graphql,
        //   so if it changes, then this must be updated, as well.
        return tx.add(normalizedFragment);
      }),
    );
    await tx.commit();
    return cachedFragments as StackPullRequestFragment[];
  }

  addComment(id: ID, body: string): Promise<AddCommentMutationData> {
    return this.client.addComment(id, body);
  }

  addLabels(input: AddLabelsToLabelableInput): Promise<AddLabelsToLabelableMutationData> {
    return this.client.addLabels(input);
  }

  addPullRequestReview(
    input: AddPullRequestReviewInput,
  ): Promise<AddPullRequestReviewMutationData> {
    return this.client.addPullRequestReview(input);
  }

  addPullRequestReviewComment(
    input: AddPullRequestReviewCommentInput,
  ): Promise<AddPullRequestReviewCommentMutationData> {
    return this.client.addPullRequestReviewComment(input);
  }

  removeLabels(
    input: RemoveLabelsFromLabelableInput,
  ): Promise<RemoveLabelsFromLabelableMutationData> {
    return this.client.removeLabels(input);
  }

  requestReviews(input: RequestReviewsInput): Promise<RequestReviewsMutationData> {
    return this.client.requestReviews(input);
  }

  submitPullRequestReview(
    input: SubmitPullRequestReviewInput,
  ): Promise<SubmitPullRequestReviewMutationData> {
    return this.client.submitPullRequestReview(input);
  }

  /**
   * Attempts to fetch the commit from the local IndexedDB. Returns null if
   * the commit could not be found in IndexedDB, though it could still exist on
   * the server.
   */
  private getCachedCommit(oid: GitObjectID): Promise<Commit | null> {
    const tx = this.db.transaction(DB_COMMIT_STORE_NAME, 'readonly');
    const store = tx.objectStore(DB_COMMIT_STORE_NAME);
    const request = store.get(oid);
    return new Promise((resolve, reject) => {
      request.onsuccess = async _event => {
        const {result: normalizedCommit} = request;
        if (normalizedCommit == null) {
          return resolve(null);
        }

        const {
          oid,
          id,
          url,
          message,
          messageHeadline,
          messageHeadlineHTML,
          messageBody,
          messageBodyHTML,
          parents,
          tree_oid,
          committedDate,
        } = normalizedCommit;
        const tree = await this.getTree(tree_oid);
        if (tree == null) {
          return reject(`tree ${tree_oid} not found for commit ${oid}`);
        }

        const commit = {
          oid: oid as GitObjectID,
          id: id as ID,
          url,
          message: message as string,
          messageHeadline: messageHeadline as string,
          messageHeadlineHTML,
          messageBody,
          messageBodyHTML,
          parents: parents as GitObjectID[],
          tree,
          committedDate,
        };
        resolve(commit);
      };
      request.onerror = reject;
    });
  }

  /**
   * Attempts to fetch the tree from the local IndexedDB. Returns null if
   * the tree could not be found in IndexedDB, though it could still exist on
   * the server.
   */
  private getCachedTree(oid: GitObjectID): Promise<Tree | null> {
    const tx = this.db.transaction(DB_TREE_STORE_NAME, 'readonly');
    const store = tx.objectStore(DB_TREE_STORE_NAME);
    const request = store.get(oid);
    return new Promise((resolve, reject) => {
      request.onsuccess = _event => {
        resolve(request.result ?? null);
      };
      request.onerror = reject;
    });
  }

  /**
   * Attempts to fetch the blob from the local IndexedDB. Returns null if
   * the blob could not be found in IndexedDB, though it could still exist on
   * the server.
   */
  private getCachedBlob(oid: GitObjectID): Promise<Blob | null> {
    const tx = this.db.transaction(DB_BLOB_STORE_NAME, 'readonly');
    const store = tx.objectStore(DB_BLOB_STORE_NAME);
    const request = store.get(oid);
    return new Promise((resolve, reject) => {
      request.onsuccess = _event => {
        resolve(request.result ?? null);
      };
      request.onerror = reject;
    });
  }

  private getCachedPRFragments(prs: number[]): Promise<Array<StackPullRequestFragment | null>> {
    const tx = this.db.transaction(PR_FRAGMENT_STORE_NAME, 'readonly');
    const store = tx.objectStore(PR_FRAGMENT_STORE_NAME);
    const {owner, name} = this.getOwnerAndName();

    return Promise.all(
      prs.map(pr => {
        const key = [owner, name, pr];
        const request = store.get(key);
        return new Promise<StackPullRequestFragment | null>((resolve, reject) => {
          request.onsuccess = _event => {
            const {result} = request;
            if (result == null) {
              resolve(null);
              return;
            }

            const {title, updatedAt, state, reviewDecision, headRefOid, numComments} = result;
            resolve({
              __typename: 'PullRequest',
              number: pr,
              title,
              updatedAt,
              state,
              reviewDecision,
              headRefOid,
              comments: {
                __typename: 'IssueCommentConnection',
                totalCount: numComments,
              },
            });
          };
          request.onerror = reject;
        });
      }),
    );
  }

  private getOwnerAndName(): {owner: string; name: string} {
    const {owner, name} = this;
    if (owner == null || name == null) {
      throw new Error('owner and name must be set in CachingGitHubClient');
    }
    return {owner, name};
  }
}

const OPEN_DATABASE_TIMEOUT_MS = 10 * 1000;

/**
 * Returns an open connection to an IDBDatabase that will close if the user
 * logs out (from any window on this origin in the same browser).
 */
export function openDatabase(): Promise<IDBDatabase> {
  return _openDatabase().then(db => {
    // If we get a "logout" event from another window, close the connection in
    // this window so that the other window can call indexedDB.deleteDatabase()
    // on all the databases.
    subscribeToLogout(() => db.close(), /* includeLogoutEventsFromThisWindow */ true);
    return db;
  });
}

async function _openDatabase(): Promise<IDBDatabase> {
  const openDatabaseRequest = new Promise((resolve, reject) => {
    const request = self.indexedDB.open(DB_NAME, DB_VERSION);
    request.onsuccess = _event => {
      resolve(request.result);
    };
    request.onerror = event => reject(event.target);
    request.onblocked = event =>
      // eslint-disable-next-line no-console
      console.error(
        `indexedDB blocked while trying to open ${DB_NAME} with version ${DB_VERSION}:`,
        event,
      );
    request.onupgradeneeded = (_event: IDBVersionChangeEvent) => {
      const {result: db} = request;
      const commitStore = db.createObjectStore(DB_COMMIT_STORE_NAME, {
        keyPath: 'oid',
        autoIncrement: false,
      });
      commitStore.createIndex('id', 'id', {unique: false});
      commitStore.createIndex('url', 'url', {unique: false});
      commitStore.createIndex('message', 'message', {unique: false});
      commitStore.createIndex('messageHeadline', 'messageHeadline', {unique: false});
      commitStore.createIndex('messageHeadlineHTML', 'messageHeadlineHTML', {unique: false});
      commitStore.createIndex('messageBody', 'messageBody', {unique: false});
      commitStore.createIndex('messageBodyHTML', 'messageBodyHTML', {unique: false});
      commitStore.createIndex('parents', 'parents', {unique: false});
      commitStore.createIndex('tree_oid', 'tree_oid', {unique: false});
      commitStore.createIndex('committedDate', 'committedDate', {unique: false});

      const treeStore = db.createObjectStore(DB_TREE_STORE_NAME, {
        keyPath: 'oid',
        autoIncrement: false,
      });
      treeStore.createIndex('id', 'id', {unique: false});
      treeStore.createIndex('entries', 'entries', {unique: false});

      const blobStore = db.createObjectStore(DB_BLOB_STORE_NAME, {
        keyPath: 'oid',
        autoIncrement: false,
      });
      blobStore.createIndex('id', 'id', {unique: false});
      blobStore.createIndex('byteSize', 'byteSize', {unique: false});
      blobStore.createIndex('isBinary', 'isBinary', {unique: false});
      blobStore.createIndex('isTruncated', 'isTruncated', {unique: false});
      blobStore.createIndex('text', 'text', {unique: false});

      const pullRequestFragmentStore = db.createObjectStore(PR_FRAGMENT_STORE_NAME, {
        keyPath: ['owner', 'name', 'number'],
        autoIncrement: false,
      });
      pullRequestFragmentStore.createIndex('title', 'title', {unique: false});
      pullRequestFragmentStore.createIndex('updatedAt', 'updatedAt', {unique: false});
      pullRequestFragmentStore.createIndex('state', 'state', {unique: false});
      pullRequestFragmentStore.createIndex('headRefOid', 'headRefOid', {unique: false});
      pullRequestFragmentStore.createIndex('numComments', 'numComments', {unique: false});
    };
  });

  // On one occasion, we saw indexedDB.open() fail to fire any of the
  // expected events (success, error, blocked, upgradeneeded) such that
  // openDatabaseRequest never settled, causing all queries into
  // CachingGitHubClient to hang forever. As a safeguard, we leverage
  // Promise.race() to introduce a timeout so force an explicit failure in this
  // case.
  //
  // Closing all of the Google Chrome browser tabs that were serving content
  // from the domain and then reopening them appeared to fix the issue.
  // Presumably there was some sort of active connection that was preventing
  // open calls from succeeding? Unclear.
  const database = await rejectAfterTimeout(
    openDatabaseRequest,
    OPEN_DATABASE_TIMEOUT_MS,
    `database failed to open within ${OPEN_DATABASE_TIMEOUT_MS}ms`,
  );
  if (database instanceof IDBDatabase) {
    return database;
  } else if (database instanceof Error) {
    throw database;
  } else {
    throw Error(`invariant failed, database object was: ${database}`);
  }
}

function normalizeCommit(commit: Commit): NormalizedCommit {
  const {
    oid,
    id,
    url,
    message,
    messageHeadline,
    messageHeadlineHTML,
    messageBody,
    messageBodyHTML,
    parents,
    committedDate,
  } = commit;
  return {
    oid,
    id,
    url,
    message,
    messageHeadline,
    messageHeadlineHTML,
    messageBody,
    messageBodyHTML,
    parents,
    tree_oid: commit.tree.oid,
    committedDate,
  };
}

function normalizePullRequestFragment(
  owner: string,
  name: string,
  fragment: StackPullRequestFragment,
): NormalizedStackPullRequestFragment {
  const {number, title, updatedAt, state, reviewDecision, headRefOid, comments} = fragment;
  return {
    owner,
    name,
    number,
    title,
    updatedAt,
    state,
    reviewDecision,
    headRefOid,
    numComments: comments.totalCount,
  };
}
