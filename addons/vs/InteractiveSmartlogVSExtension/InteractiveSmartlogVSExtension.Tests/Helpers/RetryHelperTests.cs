/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Threading.Tasks;
using InteractiveSmartlogVSExtension.Helpers;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Moq;

namespace InteractiveSmartlogVSExtension.Tests.Helpers
{
    [TestClass]
    public class RetryHelperTests
    {
        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_SucceedsFirstTry_ReturnsResult()
        {
            // Arrange
            var expectedResult = "Success";
            Func<Task<string>> operation = () => Task.FromResult(expectedResult);

            // Act
            var result = await RetryHelper.RetryWithExponentialBackoffAsync(operation);

            // Assert
            Assert.AreEqual(expectedResult, result);
        }

        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_FailsFirstThenSucceeds_ReturnsResult()
        {
            // Arrange
            var expectedResult = "Success";
            int attempts = 0;
            Func<Task<string>> operation = () =>
            {
                attempts++;
                if (attempts == 1)
                {
                    throw new Exception("First attempt failed");
                }
                return Task.FromResult(expectedResult);
            };

            // Act
            var result = await RetryHelper.RetryWithExponentialBackoffAsync(operation);

            // Assert
            Assert.AreEqual(expectedResult, result);
            Assert.AreEqual(2, attempts);
        }

        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_AlwaysFails_ThrowsLastException()
        {
            // Arrange
            var expectedMessage = "Operation failed";
            Func<Task<string>> operation = () => throw new InvalidOperationException(expectedMessage);

            // Act & Assert
            var exception = await Assert.ThrowsExceptionAsync<InvalidOperationException>(
                async () => await RetryHelper.RetryWithExponentialBackoffAsync(operation));

            Assert.AreEqual(expectedMessage, exception.Message);
        }

        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_FailsUntilLastAttempt_ReturnsResult()
        {
            // Arrange
            var expectedResult = "Success";
            int attempts = 0;
            int maxAttempts = 3;

            Func<Task<string>> operation = () =>
            {
                attempts++;
                if (attempts < maxAttempts)
                {
                    throw new Exception($"Attempt {attempts} failed");
                }
                return Task.FromResult(expectedResult);
            };

            // Act
            var result = await RetryHelper.RetryWithExponentialBackoffAsync(operation, maxAttempts);

            // Assert
            Assert.AreEqual(expectedResult, result);
            Assert.AreEqual(maxAttempts, attempts);
        }

        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_CustomMaxAttempts_RespectsMaxAttempts()
        {
            // Arrange
            int attempts = 0;
            int maxAttempts = 5;

            Func<Task<string>> operation = () =>
            {
                attempts++;
                throw new Exception($"Attempt {attempts} failed");
            };

            // Act & Assert
            await Assert.ThrowsExceptionAsync<Exception>(
                async () => await RetryHelper.RetryWithExponentialBackoffAsync(operation, maxAttempts));

            Assert.AreEqual(maxAttempts, attempts);
        }

        [TestMethod]
        public async Task RetryWithExponentialBackoffAsync_CustomInitialDelay_UsesInitialDelay()
        {
            // This test is more challenging to implement precisely without introducing timing dependencies
            // We'll focus on verifying the method doesn't throw exceptions with custom delay

            // Arrange
            var expectedResult = "Success";
            int attempts = 0;

            Func<Task<string>> operation = () =>
            {
                attempts++;
                if (attempts == 1)
                {
                    throw new Exception("First attempt failed");
                }
                return Task.FromResult(expectedResult);
            };

            // Act
            var result = await RetryHelper.RetryWithExponentialBackoffAsync(operation, initialDelayMs: 10);

            // Assert
            Assert.AreEqual(expectedResult, result);
            Assert.AreEqual(2, attempts);
        }
    }
}
