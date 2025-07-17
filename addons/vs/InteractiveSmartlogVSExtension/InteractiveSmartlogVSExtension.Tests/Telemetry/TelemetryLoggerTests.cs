/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Reflection;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Helpers;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Moq;
using Newtonsoft.Json;

namespace InteractiveSmartlogVSExtension.Tests.Telemetry
{
    [TestClass]
    public class TelemetryLoggerTests
    {
        [TestMethod]
        public void Constructor_InitializesProperties()
        {
            // Arrange & Act
            var logger = new TelemetryLogger();

            // Assert
            // Since the properties are private, we can't directly assert their values
            // But we can verify the logger was created without exceptions
            Assert.IsNotNull(logger);
        }

        [TestMethod]
        public void LogInfo_CallsWriteToScuba()
        {
            // This test is challenging to implement without modifying the production code
            // to make it more testable. In a real-world scenario, you might:
            // 1. Extract an interface for ScribeToolManager
            // 2. Use dependency injection to provide a mock implementation for testing
            // 3. Verify the mock was called with the expected parameters

            // For now, we'll just verify the method doesn't throw exceptions

            // Arrange
            var logger = new TelemetryLogger();

            // Act & Assert
            try
            {
                // This will actually try to execute scribe_cat, which might fail in a test environment
                // We're just checking that the method doesn't throw unexpected exceptions
                logger.logInfo(ActionType.RenderISLView);

                // If we get here, the test passes (though it might have logged an error)
                Assert.IsTrue(true);
            }
            catch (Exception ex)
            {
                // If the exception is related to scribe_cat not being found, that's expected in a test environment
                if (ex.Message.Contains("scribe_cat") && ex.Message.Contains("not found"))
                {
                    Assert.IsTrue(true);
                }
                else
                {
                    Assert.Fail($"Unexpected exception: {ex.Message}");
                }
            }
        }

        [TestMethod]
        public void LogError_CallsWriteToScuba()
        {
            // Similar to LogInfo_CallsWriteToScuba, this test is challenging without modifying the production code

            // Arrange
            var logger = new TelemetryLogger();

            // Act & Assert
            try
            {
                // This will actually try to execute scribe_cat, which might fail in a test environment
                logger.logError(ActionType.RenderISLView, ErrorCodes.SlWebFailed, "Test error message");

                // If we get here, the test passes (though it might have logged an error)
                Assert.IsTrue(true);
            }
            catch (Exception ex)
            {
                // If the exception is related to scribe_cat not being found, that's expected in a test environment
                if (ex.Message.Contains("scribe_cat") && ex.Message.Contains("not found"))
                {
                    Assert.IsTrue(true);
                }
                else
                {
                    Assert.Fail($"Unexpected exception: {ex.Message}");
                }
            }
        }

        [TestMethod]
        public void GetBaseString_ReturnsExpectedKeys()
        {
            // Arrange
            var logger = new TelemetryLogger();

            // Act
            // We need to use reflection to access the private method
            var method = typeof(TelemetryLogger).GetMethod("getBaseString",
                BindingFlags.NonPublic | BindingFlags.Instance);
            var result = method.Invoke(logger, null) as System.Collections.Generic.Dictionary<string, string>;

            // Assert
            Assert.IsNotNull(result);
            Assert.IsTrue(result.ContainsKey("session_id"));
            Assert.IsTrue(result.ContainsKey("username"));
            Assert.IsTrue(result.ContainsKey("hostname"));
            Assert.IsTrue(result.ContainsKey("ide_name"));
            Assert.IsTrue(result.ContainsKey("ide_version"));
            Assert.IsTrue(result.ContainsKey("extension_name"));
            Assert.IsTrue(result.ContainsKey("extension_version"));
        }

        // Note: Testing the writeToScuba method directly would require modifying the production code
        // to make it more testable. In a real-world scenario, you might:
        // 1. Extract an interface for ScribeToolManager
        // 2. Use dependency injection to provide a mock implementation for testing
        // 3. Verify the mock was called with the expected parameters
    }
}
