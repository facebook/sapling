/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {OpenISLOptions, PageOptions, TestBrowser} from './testBrowser';
import type {TestRepo} from './testRepo';

/** Reexport for convenience. */
export type {TestBrowser, TestRepo};

/** Defines an example - what does the repo looks like and what to do after opening ISL. */
export interface Example {
  /** Prepare the test repo. */
  populateRepo(repo: TestRepo): Promise<void>;

  /** Logic to run after opening the ISL page. */
  postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void>;

  /** Page options like what the initial viewport size is. */
  pageOptions(): PageOptions;

  /** Initial ISL options. */
  openISLOptions: OpenISLOptions;
}

export const BASE_EXAMPLE: Example = {
  async populateRepo(repo: TestRepo): Promise<void> {
    const now = this.openISLOptions.now ?? 0;
    await repo.cached(async repo => {
      const username = 'Mary <mary@example.com>';
      await repo.setConfig([
        `ui.username=${username}`,
        `devel.default-date=${now} 0`,
        'remotenames.selectivepulldefault=main',
        'smartlog.names=main,stable',
      ]);
      await repo.drawdag(
        `
        P9
         : C3
         | :
         | C1
         |/
        P7
         :
         | B3
         | :
         | B1
         |/
        P5
         : A2
         | |
         | A1
         |/
        P3
         :
        P1
        `,
        `
        now('${now} 0')
        commit(user='${username}')
        commit('A1', '[sl] windows: update Python', date='300h ago')
        commit('A2', 'debug', date='300h ago')
        commit('B1', '[eden] Thread EdenConfig down to Windows fsck', date='3d ago')
        commit('B2', '[eden] Remove n^2 path comparisons from Windows fsck', date='3d ago')
        commit('B3', '[edenfs] Recover Overlay from disk/scm for Windows fsck', date='3d ago')
        commit('C1', '[eden] Use PathMap for WindowsFsck', date='2d ago')
        commit('C2', '[eden] Close Windows file handle during Windows Fsck', date='2d ago')
        commit('C3', 'temp', date='2d ago')
        commit('C4', '[eden] Support long paths in Windows FSCK', date='12m ago')
        # Use different dates for public commits so ISL forceConnectPublic() can sort them.
        opts = {
            'P9': {'remotename': 'remote/main'},
            'P7': {'remotename': 'remote/stable', 'date': '48h ago'},
            'P6': {'pred': 'A1', 'op': 'land'},
            'P5': {'date': '73h ago'},
            'P3': {'date': '301h ago'},
        }
        date = '0h ago'
        for i in range(9, 0, -1):
            name = f'P{i}'
            kwargs = opts.get(name) or {}
            date = kwargs.pop('date', None) or date
            commit(name, date=date, **kwargs)
            date = str(int(date.split('h')[0]) + 1) + 'h ago'
        `,
      );
    });
  },
  async postOpenISL(browser: TestBrowser, _repo: TestRepo): Promise<void> {
    await browser.page.screenshot({path: 'example.png'});
  },
  pageOptions(): PageOptions {
    return {
      width: this.openISLOptions.sidebarOpen ? 860 : 600,
      height: 500,
    };
  },
  openISLOptions: {
    lightTheme: true,
    sidebarOpen: false,
    now: 964785600, // 2000-7-28
  },
};
