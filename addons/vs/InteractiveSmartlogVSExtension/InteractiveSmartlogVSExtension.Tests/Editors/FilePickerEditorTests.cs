/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.ComponentModel;
using System.Drawing.Design;
using System.Windows.Forms.Design;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Moq;

namespace InteractiveSmartlogVSExtension.Tests.Editors
{
    [TestClass]
    public class FilePickerEditorTests
    {
        private FilePickerEditor _editor;

        [TestInitialize]
        public void Setup()
        {
            _editor = new FilePickerEditor();
        }

        [TestMethod]
        public void GetEditStyle_ReturnsModal()
        {
            // Arrange
            var context = new Mock<ITypeDescriptorContext>();

            // Act
            var result = _editor.GetEditStyle(context.Object);

            // Assert
            Assert.AreEqual(UITypeEditorEditStyle.Modal, result);
        }

        [TestMethod]
        public void EditValue_NullServiceProvider_ReturnsOriginalValue()
        {
            // Arrange
            var context = new Mock<ITypeDescriptorContext>();
            var originalValue = "C:\\original\\path.exe";

            // Act
            var result = _editor.EditValue(context.Object, null, originalValue);

            // Assert
            Assert.AreEqual(originalValue, result);
        }

        [TestMethod]
        public void EditValue_ServiceProviderWithoutEditorService_ReturnsOriginalValue()
        {
            // Arrange
            var context = new Mock<ITypeDescriptorContext>();
            var serviceProvider = new Mock<IServiceProvider>();
            serviceProvider.Setup(sp => sp.GetService(typeof(IWindowsFormsEditorService)))
                .Returns(null);
            var originalValue = "C:\\original\\path.exe";

            // Act
            var result = _editor.EditValue(context.Object, serviceProvider.Object, originalValue);

            // Assert
            Assert.AreEqual(originalValue, result);
        }
    }
}
