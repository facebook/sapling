"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([["6377"],{57722:function(e,t,r){r.d(t,{a:function(){return o}});var l=r(89424);function o(e,t){var r=e.append("foreignObject").attr("width","100000"),o=r.append("xhtml:div");o.attr("xmlns","http://www.w3.org/1999/xhtml");var a=t.label;switch(typeof a){case"function":o.insert(a);break;case"object":o.insert(function(){return a});break;default:o.html(a)}l.bg(o,t.labelStyle),o.style("display","inline-block"),o.style("white-space","nowrap");var n=o.node().getBoundingClientRect();return r.attr("width",n.width).attr("height",n.height),r}},89424:function(e,t,r){r.d(t,{$p:function(){return d},O1:function(){return n},WR:function(){return p},bF:function(){return a},bg:function(){return c}});var l=r(66433),o=r(18782);function a(e,t){return!!e.children(t).length}function n(e){return s(e.v)+":"+s(e.w)+":"+s(e.name)}var i=/:/g;function s(e){return e?String(e).replace(i,"\\:"):""}function c(e,t){t&&e.attr("style",t)}function d(e,t,r){t&&e.attr("class",t).attr("class",r+" "+e.attr("class"))}function p(e,t){var r=t.graph();if(l.Z(r)){var a=r.transition;if(o.Z(a))return a(e)}return e}},73265:function(e,t,r){r.d(t,{Z:function(){return a}});var l=r(53763),o=r(26652);let a=(e,t)=>l.Z.lang.round(o.Z.parse(e)[t])},78495:function(e,t,r){r.d(t,{diagram:function(){return n}});var l=r(62658),o=r(64381),a=r(37278);r(84056),r(50043),r(42752),r(2024),r(27484),r(17967),r(27856);let n={parser:l.p,db:l.f,renderer:o.f,styles:o.a,init:e=>{!e.flowchart&&(e.flowchart={}),e.flowchart.arrowMarkerAbsolute=e.arrowMarkerAbsolute,(0,a.p)({flowchart:{arrowMarkerAbsolute:e.arrowMarkerAbsolute}}),o.f.setConf(e.flowchart),l.f.clear(),l.f.setGen("gen-2")}}},64381:function(e,t,r){r.d(t,{a:function(){return h},f:function(){return f}});var l=r(84056),o=r(57722),a=r(50043),n=r(19544),i=r(37278),s=r(73265),c=r(13328);let d={},p=async function(e,t,r,l,a,n){let s=l.select(`[id="${r}"]`);for(let r of Object.keys(e)){let l;let c=e[r],d="default";c.classes.length>0&&(d=c.classes.join(" ")),d+=" flowchart-label";let p=(0,i.k)(c.styles),b=void 0!==c.text?c.text:c.id;if(i.l.info("vertex",c,c.labelType),"markdown"===c.labelType)i.l.info("vertex",c,c.labelType);else if((0,i.m)((0,i.c)().flowchart.htmlLabels)){let e={label:b};(l=(0,o.a)(s,e).node()).parentNode.removeChild(l)}else{let e=a.createElementNS("http://www.w3.org/2000/svg","text");for(let t of(e.setAttribute("style",p.labelStyle.replace("color:","fill:")),b.split(i.e.lineBreakRegex))){let r=a.createElementNS("http://www.w3.org/2000/svg","tspan");r.setAttributeNS("http://www.w3.org/XML/1998/namespace","xml:space","preserve"),r.setAttribute("dy","1em"),r.setAttribute("x","1"),r.textContent=t,e.appendChild(r)}l=e}let u=0,f="";switch(c.type){case"round":u=5,f="rect";break;case"square":case"group":default:f="rect";break;case"diamond":f="question";break;case"hexagon":f="hexagon";break;case"odd":case"odd_right":f="rect_left_inv_arrow";break;case"lean_right":f="lean_right";break;case"lean_left":f="lean_left";break;case"trapezoid":f="trapezoid";break;case"inv_trapezoid":f="inv_trapezoid";break;case"circle":f="circle";break;case"ellipse":f="ellipse";break;case"stadium":f="stadium";break;case"subroutine":f="subroutine";break;case"cylinder":f="cylinder";break;case"doublecircle":f="doublecircle"}let w=await (0,i.r)(b,(0,i.c)());t.setNode(c.id,{labelStyle:p.labelStyle,shape:f,labelText:w,labelType:c.labelType,rx:u,ry:u,class:d,style:p.style,id:c.id,link:c.link,linkTarget:c.linkTarget,tooltip:n.db.getTooltip(c.id)||"",domId:n.db.lookUpDomId(c.id),haveCallback:c.haveCallback,width:"group"===c.type?500:void 0,dir:c.dir,type:c.type,props:c.props,padding:(0,i.c)().flowchart.padding}),i.l.info("setNode",{labelStyle:p.labelStyle,labelType:c.labelType,shape:f,labelText:w,rx:u,ry:u,class:d,style:p.style,id:c.id,domId:n.db.lookUpDomId(c.id),width:"group"===c.type?500:void 0,type:c.type,dir:c.dir,props:c.props,padding:(0,i.c)().flowchart.padding})}},b=async function(e,t,r){let o,a;i.l.info("abc78 edges = ",e);let n=0,s={};if(void 0!==e.defaultStyle){let t=(0,i.k)(e.defaultStyle);o=t.style,a=t.labelStyle}for(let r of e){n++;let c="L-"+r.start+"-"+r.end;void 0===s[c]?s[c]=0:s[c]++,i.l.info("abc78 new entry",c,s[c]);let p=c+"-"+s[c];i.l.info("abc78 new link id to be used is",c,p,s[c]);let b="LS-"+r.start,u="LE-"+r.end,f={style:"",labelStyle:""};switch(f.minlen=r.length||1,"arrow_open"===r.type?f.arrowhead="none":f.arrowhead="normal",f.arrowTypeStart="arrow_open",f.arrowTypeEnd="arrow_open",r.type){case"double_arrow_cross":f.arrowTypeStart="arrow_cross";case"arrow_cross":f.arrowTypeEnd="arrow_cross";break;case"double_arrow_point":f.arrowTypeStart="arrow_point";case"arrow_point":f.arrowTypeEnd="arrow_point";break;case"double_arrow_circle":f.arrowTypeStart="arrow_circle";case"arrow_circle":f.arrowTypeEnd="arrow_circle"}let w="",h="";switch(r.stroke){case"normal":w="fill:none;",void 0!==o&&(w=o),void 0!==a&&(h=a),f.thickness="normal",f.pattern="solid";break;case"dotted":f.thickness="normal",f.pattern="dotted",f.style="fill:none;stroke-width:2px;stroke-dasharray:3;";break;case"thick":f.thickness="thick",f.pattern="solid",f.style="stroke-width: 3.5px;fill:none;";break;case"invisible":f.thickness="invisible",f.pattern="solid",f.style="stroke-width: 0;fill:none;"}if(void 0!==r.style){let e=(0,i.k)(r.style);w=e.style,h=e.labelStyle}f.style=f.style+=w,f.labelStyle=f.labelStyle+=h,void 0!==r.interpolate?f.curve=(0,i.n)(r.interpolate,l.c_6):void 0!==e.defaultInterpolate?f.curve=(0,i.n)(e.defaultInterpolate,l.c_6):f.curve=(0,i.n)(d.curve,l.c_6),void 0===r.text?void 0!==r.style&&(f.arrowheadStyle="fill: #333"):(f.arrowheadStyle="fill: #333",f.labelpos="c"),f.labelType=r.labelType,f.label=await (0,i.r)(r.text.replace(i.e.lineBreakRegex,"\n"),(0,i.c)()),void 0===r.style&&(f.style=f.style||"stroke: #333; stroke-width: 1.5px;fill:none;"),f.labelStyle=f.labelStyle.replace("color:","fill:"),f.id=p,f.classes="flowchart-link "+b+" "+u,t.setEdge(r.start,r.end,f,n)}},u=async function(e,t,r,o){let s,c;i.l.info("Drawing flowchart");let d=o.db.getDirection();void 0===d&&(d="TD");let{securityLevel:u,flowchart:f}=(0,i.c)(),w=f.nodeSpacing||50,h=f.rankSpacing||50;"sandbox"===u&&(s=(0,l.Ys)("#i"+t));let g="sandbox"===u?(0,l.Ys)(s.nodes()[0].contentDocument.body):(0,l.Ys)("body"),y="sandbox"===u?s.nodes()[0].contentDocument:document,k=new a.k({multigraph:!0,compound:!0}).setGraph({rankdir:d,nodesep:w,ranksep:h,marginx:0,marginy:0}).setDefaultEdgeLabel(function(){return{}}),x=o.db.getSubGraphs();i.l.info("Subgraphs - ",x);for(let e=x.length-1;e>=0;e--)c=x[e],i.l.info("Subgraph - ",c),o.db.addVertex(c.id,{text:c.title,type:c.labelType},"group",void 0,c.classes,c.dir);let v=o.db.getVertices(),m=o.db.getEdges();i.l.info("Edges",m);let S=0;for(S=x.length-1;S>=0;S--){c=x[S],(0,l.td_)("cluster").append("text");for(let e=0;e<c.nodes.length;e++)i.l.info("Setting up subgraphs",c.nodes[e],c.id),k.setParent(c.nodes[e],c.id)}await p(v,k,t,g,y,o),await b(m,k);let T=g.select(`[id="${t}"]`),_=g.select("#"+t+" g");if(await (0,n.r)(_,k,["point","circle","cross"],"flowchart",t),i.u.insertTitle(T,"flowchartTitleText",f.titleTopMargin,o.db.getDiagramTitle()),(0,i.o)(k,T,f.diagramPadding,f.useMaxWidth),o.db.indexNodes("subGraph"+S),!f.htmlLabels)for(let e of y.querySelectorAll('[id="'+t+'"] .edgeLabel .label')){let t=e.getBBox(),r=y.createElementNS("http://www.w3.org/2000/svg","rect");r.setAttribute("rx",0),r.setAttribute("ry",0),r.setAttribute("width",t.width),r.setAttribute("height",t.height),e.insertBefore(r,e.firstChild)}Object.keys(v).forEach(function(e){let r=v[e];if(r.link){let o=(0,l.Ys)("#"+t+' [id="'+e+'"]');if(o){let e=y.createElementNS("http://www.w3.org/2000/svg","a");e.setAttributeNS("http://www.w3.org/2000/svg","class",r.classes.join(" ")),e.setAttributeNS("http://www.w3.org/2000/svg","href",r.link),e.setAttributeNS("http://www.w3.org/2000/svg","rel","noopener"),"sandbox"===u?e.setAttributeNS("http://www.w3.org/2000/svg","target","_top"):r.linkTarget&&e.setAttributeNS("http://www.w3.org/2000/svg","target",r.linkTarget);let t=o.insert(function(){return e},":first-child"),l=o.select(".label-container");l&&t.append(function(){return l.node()});let a=o.select(".label");a&&t.append(function(){return a.node()})}}})},f={setConf:function(e){for(let t of Object.keys(e))d[t]=e[t]},addVertices:p,addEdges:b,getClasses:function(e,t){return t.db.getClasses()},draw:u},w=(e,t)=>{let r=s.Z,l=r(e,"r"),o=r(e,"g"),a=r(e,"b");return c.Z(l,o,a,t)},h=e=>`.label {
    font-family: ${e.fontFamily};
    color: ${e.nodeTextColor||e.textColor};
  }
  .cluster-label text {
    fill: ${e.titleColor};
  }
  .cluster-label span,p {
    color: ${e.titleColor};
  }

  .label text,span,p {
    fill: ${e.nodeTextColor||e.textColor};
    color: ${e.nodeTextColor||e.textColor};
  }

  .node rect,
  .node circle,
  .node ellipse,
  .node polygon,
  .node path {
    fill: ${e.mainBkg};
    stroke: ${e.nodeBorder};
    stroke-width: 1px;
  }
  .flowchart-label text {
    text-anchor: middle;
  }
  // .flowchart-label .text-outer-tspan {
  //   text-anchor: middle;
  // }
  // .flowchart-label .text-inner-tspan {
  //   text-anchor: start;
  // }

  .node .katex path {
    fill: #000;
    stroke: #000;
    stroke-width: 1px;
  }

  .node .label {
    text-align: center;
  }
  .node.clickable {
    cursor: pointer;
  }

  .arrowheadPath {
    fill: ${e.arrowheadColor};
  }

  .edgePath .path {
    stroke: ${e.lineColor};
    stroke-width: 2.0px;
  }

  .flowchart-link {
    stroke: ${e.lineColor};
    fill: none;
  }

  .edgeLabel {
    background-color: ${e.edgeLabelBackground};
    rect {
      opacity: 0.5;
      background-color: ${e.edgeLabelBackground};
      fill: ${e.edgeLabelBackground};
    }
    text-align: center;
  }

  /* For html labels only */
  .labelBkg {
    background-color: ${w(e.edgeLabelBackground,.5)};
    // background-color: 
  }

  .cluster rect {
    fill: ${e.clusterBkg};
    stroke: ${e.clusterBorder};
    stroke-width: 1px;
  }

  .cluster text {
    fill: ${e.titleColor};
  }

  .cluster span,p {
    color: ${e.titleColor};
  }
  /* .cluster div {
    color: ${e.titleColor};
  } */

  div.mermaidTooltip {
    position: absolute;
    text-align: center;
    max-width: 200px;
    padding: 2px;
    font-family: ${e.fontFamily};
    font-size: 12px;
    background: ${e.tertiaryColor};
    border: 1px solid ${e.border2};
    border-radius: 2px;
    pointer-events: none;
    z-index: 100;
  }

  .flowchartTitleText {
    text-anchor: middle;
    font-size: 18px;
    fill: ${e.textColor};
  }
`}}]);