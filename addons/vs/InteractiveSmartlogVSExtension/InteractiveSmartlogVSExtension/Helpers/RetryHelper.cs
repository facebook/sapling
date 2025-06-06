/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Threading.Tasks;

namespace InteractiveSmartlogVSExtension.Helpers
{
    public class RetryHelper
    {
        public static async Task<T> RetryWithExponentialBackoffAsync<T>(Func<Task<T>> operation, int maxAttempts = 3, int initialDelayMs = 500)
        {
            int attempt = 0;
            int delay = initialDelayMs;
            Exception lastException = null;

            while (attempt < maxAttempts)
            {
                try
                {
                    return await operation();
                }
                catch (Exception ex)
                {
                    lastException = ex;
                    attempt++;
                    if (attempt < maxAttempts)
                        await Task.Delay(delay);
                    delay *= 2; // Exponential backoff
                }
            }
            throw lastException;
        }
    }
}
