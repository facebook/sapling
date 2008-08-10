// branch_renderer.js - Rendering of branch DAGs on the client side
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
	if (navigator.userAgent.indexOf('MSIE') >= 0) this.canvas = window.G_vmlCanvasManager.initElement(this.canvas);
	this.ctx = this.canvas.getContext('2d');
	this.ctx.strokeStyle = 'rgb(0, 0, 0)';
	this.ctx.fillStyle = 'rgb(0, 0, 0)';
	this.cur = [0, 0];
	this.line_width = 3;
	this.bg = [0, 4];
	this.cell = [2, 0];
	this.columns = 0;
	this.revlink = '';
	
	this.scale = function(height) {
		this.bg_height = height;
		this.box_size = Math.floor(this.bg_height / 1.2);
		this.cell_height = this.box_size;
	}
	
	function colorPart(num) {
		num *= 255
		num = num < 0 ? 0 : num;
		num = num > 255 ? 255 : num;
		var digits = Math.round(num).toString(16);
		if (num < 16) {
			return '0' + digits;
		} else {
			return digits;
		}
	}

	this.setColor = function(color, bg, fg) {
		
		// Set the colour.
		//
		// Picks a distinct colour based on an internal wheel; the bg
		// parameter provides the value that should be assigned to the 'zero'
		// colours and the fg parameter provides the multiplier that should be
		// applied to the foreground colours.
		
		color %= colors.length;
		var red = (colors[color][0] * fg) || bg;
		var green = (colors[color][1] * fg) || bg;
		var blue = (colors[color][2] * fg) || bg;
		red = Math.round(red * 255);
		green = Math.round(green * 255);
		blue = Math.round(blue * 255);
		var s = 'rgb(' + red + ', ' + green + ', ' + blue + ')';
		this.ctx.strokeStyle = s;
		this.ctx.fillStyle = s;
		return s;
		
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
			
			for (var j in edges) {
				
				line = edges[j];
				start = line[0];
				end = line[1];
				color = line[2];

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
				
				this.edge(x0, y0, x1, y1, color);
				
			}
			
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
