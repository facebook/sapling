/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.ComponentModel;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Converters
{
    [TestClass]
    public class DiffToolConverterTests
    {
        private DiffToolConverter _converter;

        [TestInitialize]
        public void Setup()
        {
            _converter = new DiffToolConverter(typeof(DiffTool));
        }

        [TestMethod]
        public void ConvertTo_EnumWithDescription_ReturnsDescription()
        {
            // Arrange
            var value = DiffTool.VisualStudio;

            // Act
            var result = _converter.ConvertTo(null, null, value, typeof(string));

            // Assert
            Assert.AreEqual("Visual Studio (internal)", result);
        }

        [TestMethod]
        public void ConvertTo_EnumWithoutDescription_ReturnsEnumName()
        {
            // Arrange
            var value = DiffTool.p4merge;

            // Act
            var result = _converter.ConvertTo(null, null, value, typeof(string));

            // Assert
            Assert.AreEqual("p4merge", result);
        }

        [TestMethod]
        public void ConvertFrom_StringMatchingDescription_ReturnsEnum()
        {
            // Arrange
            var value = "Visual Studio (internal)";

            // Act
            var result = _converter.ConvertFrom(null, null, value);

            // Assert
            Assert.AreEqual(DiffTool.VisualStudio, result);
        }

        [TestMethod]
        public void ConvertFrom_StringMatchingEnumName_ReturnsEnum()
        {
            // Arrange
            var value = "p4merge";

            // Act
            var result = _converter.ConvertFrom(null, null, value);

            // Assert
            Assert.AreEqual(DiffTool.p4merge, result);
        }

        [TestMethod]
        public void ConvertFrom_InvalidString_ReturnsVisualStudio()
        {
            // Arrange
            var value = "InvalidDiffTool";

            // Act
            var result = _converter.ConvertFrom(null, null, value);

            // Assert
            Assert.AreEqual(DiffTool.VisualStudio, result);
        }

        [TestMethod]
        public void ConvertFrom_NonStringValue_ReturnsVisualStudio()
        {
            // Arrange
            var value = 123;

            // Act
            var result = _converter.ConvertFrom(null, null, value);

            // Assert
            Assert.AreEqual(DiffTool.VisualStudio, result);
        }
    }
}
