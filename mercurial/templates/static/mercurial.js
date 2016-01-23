// mercurial.js - JavaScript utility functions
//
// Rendering of branch DAGs on the client side
// Display of elapsed time
// Show or hide diffstat
//
// Copyright 2008 Dirkjan Ochtman <dirkjan AT ochtman DOT nl>
// Copyright 2006 Alexander Schremmer <alex AT alexanderweb DOT de>
//
// derived from code written by Scott James Remnant <scott@ubuntu.com>
// Copyright 2005 Canonical Ltd.
//
// This software may be used and distributed according to the terms
// of the GNU General Public License, incorporated herein by reference.

var colors = [
	[ 1.0, 0.0, 0.0 ],
	[ 1.0, 1.0, 0.0 ],
	[ 0.0, 1.0, 0.0 ],
	[ 0.0, 1.0, 1.0 ],
	[ 0.0, 0.0, 1.0 ],
	[ 1.0, 0.0, 1.0 ]
];

function Graph() {

	this.canvas = document.getElementById('graph');
	if (window.G_vmlCanvasManager) this.canvas = window.G_vmlCanvasManager.initElement(this.canvas);
	this.ctx = this.canvas.getContext('2d');
	this.ctx.strokeStyle = 'rgb(0, 0, 0)';
	this.ctx.fillStyle = 'rgb(0, 0, 0)';
	this.cur = [0, 0];
	this.line_width = 3;
	this.bg = [0, 4];
	this.cell = [2, 0];
	this.columns = 0;
	this.revlink = '';

	this.reset = function() {
		this.bg = [0, 4];
		this.cell = [2, 0];
		this.columns = 0;
		document.getElementById('nodebgs').innerHTML = '';
		document.getElementById('graphnodes').innerHTML = '';
	}

	this.scale = function(height) {
		this.bg_height = height;
		this.box_size = Math.floor(this.bg_height / 1.2);
		this.cell_height = this.box_size;
	}

	this.setColor = function(color, bg, fg) {

		// Set the colour.
		//
		// If color is a string, expect an hexadecimal RGB
		// value and apply it unchanged. If color is a number,
		// pick a distinct colour based on an internal wheel;
		// the bg parameter provides the value that should be
		// assigned to the 'zero' colours and the fg parameter
		// provides the multiplier that should be applied to
		// the foreground colours.
		var s;
		if(typeof color == "string") {
			s = "#" + color;
		} else { //typeof color == "number"
			color %= colors.length;
			var red = (colors[color][0] * fg) || bg;
			var green = (colors[color][1] * fg) || bg;
			var blue = (colors[color][2] * fg) || bg;
			red = Math.round(red * 255);
			green = Math.round(green * 255);
			blue = Math.round(blue * 255);
			s = 'rgb(' + red + ', ' + green + ', ' + blue + ')';
		}
		this.ctx.strokeStyle = s;
		this.ctx.fillStyle = s;
		return s;

	}

	this.edge = function(x0, y0, x1, y1, color, width) {

		this.setColor(color, 0.0, 0.65);
		if(width >= 0)
			 this.ctx.lineWidth = width;
		this.ctx.beginPath();
		this.ctx.moveTo(x0, y0);
		this.ctx.lineTo(x1, y1);
		this.ctx.stroke();

	}

	this.render = function(data) {

		var backgrounds = '';
		var nodedata = '';

		for (var i in data) {

			var parity = i % 2;
			this.cell[1] += this.bg_height;
			this.bg[1] += this.bg_height;

			var cur = data[i];
			var node = cur[1];
			var edges = cur[2];
			var fold = false;

			var prevWidth = this.ctx.lineWidth;
			for (var j in edges) {

				line = edges[j];
				start = line[0];
				end = line[1];
				color = line[2];
				var width = line[3];
				if(width < 0)
					 width = prevWidth;
				var branchcolor = line[4];
				if(branchcolor)
					color = branchcolor;

				if (end > this.columns || start > this.columns) {
					this.columns += 1;
				}

				if (start == this.columns && start > end) {
					var fold = true;
				}

				x0 = this.cell[0] + this.box_size * start + this.box_size / 2;
				y0 = this.bg[1] - this.bg_height / 2;
				x1 = this.cell[0] + this.box_size * end + this.box_size / 2;
				y1 = this.bg[1] + this.bg_height / 2;

				this.edge(x0, y0, x1, y1, color, width);

			}
			this.ctx.lineWidth = prevWidth;

			// Draw the revision node in the right column

			column = node[0]
			color = node[1]

			radius = this.box_size / 8;
			x = this.cell[0] + this.box_size * column + this.box_size / 2;
			y = this.bg[1] - this.bg_height / 2;
			var add = this.vertex(x, y, color, parity, cur);
			backgrounds += add[0];
			nodedata += add[1];

			if (fold) this.columns -= 1;

		}

		document.getElementById('nodebgs').innerHTML += backgrounds;
		document.getElementById('graphnodes').innerHTML += nodedata;

	}

}


function process_dates(parentSelector){

	// derived from code from mercurial/templatefilter.py

	var scales = {
		'year':  365 * 24 * 60 * 60,
		'month':  30 * 24 * 60 * 60,
		'week':    7 * 24 * 60 * 60,
		'day':    24 * 60 * 60,
		'hour':   60 * 60,
		'minute': 60,
		'second': 1
	};

	function format(count, string){
		var ret = count + ' ' + string;
		if (count > 1){
			ret = ret + 's';
		}
 		return ret;
 	}

	function shortdate(date){
		var ret = date.getFullYear() + '-';
		// getMonth() gives a 0-11 result
		var month = date.getMonth() + 1;
		if (month <= 9){
			ret += '0' + month;
		} else {
			ret += month;
		}
		ret += '-';
		var day = date.getDate();
		if (day <= 9){
			ret += '0' + day;
		} else {
			ret += day;
		}
		return ret;
	}

 	function age(datestr){
 		var now = new Date();
 		var once = new Date(datestr);
		if (isNaN(once.getTime())){
			// parsing error
			return datestr;
		}

		var delta = Math.floor((now.getTime() - once.getTime()) / 1000);

		var future = false;
		if (delta < 0){
			future = true;
			delta = -delta;
			if (delta > (30 * scales.year)){
				return "in the distant future";
			}
		}

		if (delta > (2 * scales.year)){
			return shortdate(once);
		}

		for (unit in scales){
			var s = scales[unit];
			var n = Math.floor(delta / s);
			if ((n >= 2) || (s == 1)){
				if (future){
					return format(n, unit) + ' from now';
				} else {
					return format(n, unit) + ' ago';
				}
			}
		}
	}

	var nodes = document.querySelectorAll((parentSelector || '') + ' .age');
	var dateclass = new RegExp('\\bdate\\b');
	for (var i=0; i<nodes.length; ++i){
		var node = nodes[i];
		var classes = node.className;
		var agevalue = age(node.textContent);
		if (dateclass.test(classes)){
			// We want both: date + (age)
			node.textContent += ' ('+agevalue+')';
		} else {
			node.title = node.textContent;
			node.textContent = agevalue;
		}
	}
}

function toggleDiffstat() {
    var curdetails = document.getElementById('diffstatdetails').style.display;
    var curexpand = curdetails == 'none' ? 'inline' : 'none';
    document.getElementById('diffstatdetails').style.display = curexpand;
    document.getElementById('diffstatexpand').style.display = curdetails;
}

function toggleLinewrap() {
    function getLinewrap() {
        var nodes = document.getElementsByClassName('sourcelines');
        // if there are no such nodes, error is thrown here
        return nodes[0].classList.contains('wrap');
    }

    function setLinewrap(enable) {
        var nodes = document.getElementsByClassName('sourcelines');
        for (var i = 0; i < nodes.length; i++) {
            if (enable) {
                nodes[i].classList.add('wrap');
            } else {
                nodes[i].classList.remove('wrap');
            }
        }

        var links = document.getElementsByClassName('linewraplink');
        for (var i = 0; i < links.length; i++) {
            links[i].innerHTML = enable ? 'on' : 'off';
        }
    }

    setLinewrap(!getLinewrap());
}

function format(str, replacements) {
    return str.replace(/%(\w+)%/g, function(match, p1) {
        return String(replacements[p1]);
    });
}

function makeRequest(url, method, onstart, onsuccess, onerror, oncomplete) {
    xfr = new XMLHttpRequest();
    xfr.onreadystatechange = function() {
        if (xfr.readyState === 4) {
            try {
                if (xfr.status === 200) {
                    onsuccess(xfr.responseText);
                } else {
                    throw 'server error';
                }
            } catch (e) {
                onerror(e);
            } finally {
                oncomplete();
            }
        }
    };

    xfr.open(method, url);
    xfr.overrideMimeType("text/xhtml; charset=" + document.characterSet.toLowerCase());
    xfr.send();
    onstart();
    return xfr;
}

function removeByClassName(className) {
    var nodes = document.getElementsByClassName(className);
    while (nodes.length) {
        nodes[0].parentNode.removeChild(nodes[0]);
    }
}

function docFromHTML(html) {
    var doc = document.implementation.createHTMLDocument('');
    doc.documentElement.innerHTML = html;
    return doc;
}

function appendFormatHTML(element, formatStr, replacements) {
    element.insertAdjacentHTML('beforeend', format(formatStr, replacements));
}

function ajaxScrollInit(urlFormat,
                        nextPageVar,
                        nextPageVarGet,
                        containerSelector,
                        messageFormat,
                        mode) {
    updateInitiated = false;
    container = document.querySelector(containerSelector);

    function scrollHandler() {
        if (updateInitiated) {
            return;
        }

        var scrollHeight = document.documentElement.scrollHeight;
        var clientHeight = document.documentElement.clientHeight;
        var scrollTop = document.body.scrollTop
            || document.documentElement.scrollTop;

        if (scrollHeight - (scrollTop + clientHeight) < 50) {
            updateInitiated = true;
            removeByClassName('scroll-loading-error');
            container.lastElementChild.classList.add('scroll-separator');

            if (!nextPageVar) {
                var message = {
                    'class': 'scroll-loading-info',
                    text: 'No more entries'
                };
                appendFormatHTML(container, messageFormat, message);
                return;
            }

            makeRequest(
                format(urlFormat, {next: nextPageVar}),
                'GET',
                function onstart() {
                    var message = {
                        'class': 'scroll-loading',
                        text: 'Loading...'
                    };
                    appendFormatHTML(container, messageFormat, message);
                },
                function onsuccess(htmlText) {
                    if (mode == 'graph') {
                        var sizes = htmlText.match(/^\s*<canvas id="graph" width="(\d+)" height="(\d+)"><\/canvas>$/m);
                        var addWidth = sizes[1];
                        var addHeight = sizes[2];
                        addWidth = parseInt(addWidth);
                        addHeight = parseInt(addHeight);
                        graph.canvas.width = addWidth;
                        graph.canvas.height = addHeight;

                        var dataStr = htmlText.match(/^\s*var data = (.*);$/m)[1];
                        var data = JSON.parse(dataStr);
                        if (data.length < nextPageVar) {
                            nextPageVar = undefined;
                        }
                        graph.reset();
                        graph.render(data);
                    } else {
                        var doc = docFromHTML(htmlText);
                        var nodes = doc.querySelector(containerSelector).children;
                        var curClass = 'c' + Date.now();
                        while (nodes.length) {
                            var node = nodes[0];
                            node = document.adoptNode(node);
                            node.classList.add(curClass);
                            container.appendChild(node);
                        }
                        process_dates('.' + curClass);
                    }

                    nextPageVar = nextPageVarGet(htmlText, nextPageVar);
                },
                function onerror(errorText) {
                    var message = {
                        'class': 'scroll-loading-error',
                        text: 'Error: ' + errorText
                    };
                    appendFormatHTML(container, messageFormat, message);
                },
                function oncomplete() {
                    removeByClassName('scroll-loading');
                    updateInitiated = false;
                    scrollHandler();
                }
            );
        }
    }

    window.addEventListener('scroll', scrollHandler);
    window.addEventListener('resize', scrollHandler);
    scrollHandler();
}
