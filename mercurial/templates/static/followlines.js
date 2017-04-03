// followlines.js - JavaScript utilities for followlines UI
//
// Copyright 2017 Logilab SA <contact@logilab.fr>
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//** Install event listeners for line block selection and followlines action */
document.addEventListener('DOMContentLoaded', function() {
    var sourcelines = document.getElementsByClassName('sourcelines')[0];
    if (typeof sourcelines === 'undefined') {
        return;
    }
    // URL to complement with "linerange" query parameter
    var targetUri = sourcelines.dataset.logurl;
    if (typeof targetUri === 'undefined') {
        return;
    }

    // retrieve all direct <span> children of <pre class="sourcelines">
    var spans = Array.prototype.filter.call(
        sourcelines.children,
        function(x) { return x.tagName === 'SPAN' });

    // add a "followlines-select" class to change cursor type in CSS
    for (var i = 0; i < spans.length; i++) {
        spans[i].classList.add('followlines-select');
    }

    var lineSelectedCSSClass = 'followlines-selected';

    //** add CSS class on <span> element in `from`-`to` line range */
    function addSelectedCSSClass(from, to) {
        for (var i = from; i <= to; i++) {
            spans[i].classList.add(lineSelectedCSSClass);
        }
    }

    //** remove CSS class from previously selected lines */
    function removeSelectedCSSClass() {
        var elements = sourcelines.getElementsByClassName(
            lineSelectedCSSClass);
        while (elements.length) {
            elements[0].classList.remove(lineSelectedCSSClass);
        }
    }

    // ** return the <span> element parent of `element` */
    function findParentSpan(element) {
        var parent = element.parentElement;
        if (parent === null) {
            return null;
        }
        if (element.tagName == 'SPAN' && parent.isSameNode(sourcelines)) {
            return element;
        }
        return findParentSpan(parent);
    }

    //** event handler for "click" on the first line of a block */
    function lineSelectStart(e) {
        var startElement = findParentSpan(e.target);
        if (startElement === null) {
            // not a <span> (maybe <a>): abort, keeping event listener
            // registered for other click with <span> target
            return;
        }
        var startId = parseInt(startElement.id.slice(1));
        startElement.classList.add(lineSelectedCSSClass); // CSS

        // remove this event listener
        sourcelines.removeEventListener('click', lineSelectStart);

        //** event handler for "click" on the last line of the block */
        function lineSelectEnd(e) {
            var endElement = findParentSpan(e.target);
            if (endElement === null) {
                // not a <span> (maybe <a>): abort, keeping event listener
                // registered for other click with <span> target
                return;
            }

            // remove this event listener
            sourcelines.removeEventListener('click', lineSelectEnd);

            // compute line range (startId, endId)
            var endId = parseInt(endElement.id.slice(1));
            if (endId == startId) {
                // clicked twice the same line, cancel and reset initial state
                // (CSS and event listener for selection start)
                removeSelectedCSSClass();
                sourcelines.addEventListener('click', lineSelectStart);
                return;
            }
            var inviteElement = endElement;
            if (endId < startId) {
                var tmp = endId;
                endId = startId;
                startId = tmp;
                inviteElement = startElement;
            }

            addSelectedCSSClass(startId - 1, endId -1);  // CSS

            // append the <div id="followlines"> element to last line of the
            // selection block
            var divAndButton = followlinesBox(targetUri, startId, endId);
            var div = divAndButton[0],
                button = divAndButton[1];
            inviteElement.appendChild(div);

            //** event handler for cancelling selection */
            function cancel() {
                // remove invite box
                div.parentNode.removeChild(div);
                // restore initial event listeners
                sourcelines.addEventListener('click', lineSelectStart);
                sourcelines.removeEventListener('click', cancel);
                // remove styles on selected lines
                removeSelectedCSSClass();
            }

            // bind cancel event to click on <button>
            button.addEventListener('click', cancel);
            // as well as on an click on any source line
            sourcelines.addEventListener('click', cancel);
        }

        sourcelines.addEventListener('click', lineSelectEnd);

    }

    sourcelines.addEventListener('click', lineSelectStart);

    //** return a <div id="followlines"> and inner cancel <button> elements */
    function followlinesBox(targetUri, fromline, toline) {
        // <div id="followlines">
        var div = document.createElement('div');
        div.id = 'followlines';

        //   <div class="followlines-cancel">
        var buttonDiv = document.createElement('div');
        buttonDiv.classList.add('followlines-cancel');

        //     <button>x</button>
        var button = document.createElement('button');
        button.textContent = 'x';
        buttonDiv.appendChild(button);
        div.appendChild(buttonDiv);

        //   <div class="followlines-link">
        var aDiv = document.createElement('div');
        aDiv.classList.add('followlines-link');

        //     <a href="/log/<rev>/<file>?patch=&linerange=...">
        var a = document.createElement('a');
        var url = targetUri + '?patch=&linerange=' + fromline + ':' + toline;
        a.setAttribute('href', url);
        a.textContent = 'follow lines ' + fromline + ':' + toline;
        aDiv.appendChild(a);
        div.appendChild(aDiv);

        return [div, button];
    }

}, false);
