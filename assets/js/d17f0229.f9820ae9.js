(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[8932],{39672:(e,n,t)=>{"use strict";t.r(n),t.d(n,{assets:()=>Z,contentTitle:()=>V,default:()=>O,frontMatter:()=>$,metadata:()=>F,toc:()=>R});var a=t(83117),r=t(37446),i=t(67294),l=t(3905),o=t(40460),s=t.n(o),d=t(10251),m=t.n(d);let c;const p=new(t(29643).WU);function u(e){let{initValue:n,style:a,padding:r=10,onDagParentsChange:l}=e;const[o,d]=i.useState((()=>({input:(null!=n?n:"").replace(/^\n+|\s+$/g,""),parents:new Map,comments:"",bindings:null,dropped:!1})));i.useEffect((()=>(async function(){return await p.runExclusive((async()=>{if(c)return c;const e=await t.e(8316).then(t.bind(t,28316));return await e.default(),c=e,e}))}().then((e=>{o.bindings||o.dropped||d((n=>({...n,bindings:e})))})).catch(console.error),function(){o.dropped=!0})),[]),i.useEffect((()=>{const{bindings:e,input:n,dropped:t,comments:a}=o;if(e&&!t){if(l)try{const t=n.replace(/#.*$/gm,""),r=n.replace(/.*#$/gm,""),i=e.drawdag(t);m()(o.parents,i)&&a===r||(l({parents:i,bindings:e,input:n}),o.parents=i,o.comments=r)}catch(r){console.error(r)}return function(){o.dropped=!0}}}),[o.bindings,o.input]);const u={background:"var(--ifm-color-emphasis-100)",borderRadius:"var(--ifm-global-radius)",fontFamily:"var(--ifm-font-family-monospace)",lineHeight:1,...a};return i.createElement(s(),{value:o.input,highlight:e=>e,padding:r,style:u,onValueChange:function(e){d((n=>n.input===e&&n.parents?n:{...n,input:e}))}})}const g=1,h=2,x=4,f=8,w=16,v=32,y=64,b=128,C=256,D=512,N=1024,k=2048;function E(){return i.createElement("div",null)}function A(e){let{dag:n,subset:t,style:a,circleRadius:r=16,padLineHeight:l=4,linkLineHeight:o=10,columnWidth:s=14,padding:d=4,bypassSize:m=4,dashArray:c="4,2",rotate:p=!0,drawExtra:u}=e;const A=[],B=[],M=[],S=new Map,T=(e,n)=>p?-n+" "+-e:e+" "+n,I=(e,n)=>p?[-n,-e]:[e,n],$=r,V=s;let F=r,Z=0,R=0,Y=0,O=0,W=d,q=d;function z(e,n){e>R&&(R=e+d),e<O&&(O=e-d),n>Z&&(Z=n+d),n<Y&&(Y=n+d)}function L(){W+=2*V,z(W,q)}function P(){z(W,q),q+=2*F}function H(e,n,t,a,r){const[l,o]=I(W+(e+1)*V,q+(n+1)*F),[s,d]=I(W+(t+1)*V,q+(a+1)*F),m=r?c:null,p=W+"."+q+"."+e+"."+n+"."+t+"."+a;let u="";if(o===d||l===s)u="M "+l+" "+o+" L "+s+" "+d;else{const[e,n]=I(W+V,q+F);u="M "+l+" "+o+" Q "+e+" "+n+", "+s+" "+d}B.push(i.createElement("path",{d:u,key:p,strokeDasharray:m}))}function X(e){const n=e?c:null;B.push(i.createElement("path",{d:"M "+T(W+V,q)+" l "+T(0,F-m)+" q "+T(m,m)+", "+T(0,2*m)+" l "+T(0,F-m),strokeDasharray:n,key:"b"+W+"."+q}))}function _(e){const[n,t]=I(W+V,q+F);A.push(i.createElement("circle",{cx:n,cy:t,r:$,key:e})),M.push(i.createElement("text",{x:n,y:t,textAnchor:"middle",alignmentBaseline:"middle",key:e},e)),S.set(e,{cx:n,cy:t,name:e})}function j(e,n){void 0===n&&(n=null);const t=e.some((e=>"Ancestor"==e));F=n?r:t?o:l,W=d;for(const a of e){switch(a){case"Ancestor":H(0,-1,0,1,!0);break;case"Parent":H(0,-1,0,1);break;case"Node":_(n)}L()}P()}function G(e){F=o,W=d;for(const n of e){const{bits:t}=n;function a(e,n,a){0!=(t&(e|n))&&a(0!=(t&n))}a(g,h,(e=>{H(-1,0,1,0,e)})),a(C,D,(e=>{H(-1,0,0,-1,e)})),a(N,k,(e=>{H(1,0,0,-1,e)})),a(w,v,(e=>{H(-1,0,0,1,e)})),a(y,b,(e=>{H(1,0,0,1,e)})),a(x,f,(e=>{t&(g|h)?X(e):H(0,-1,0,1,e)})),L()}P()}if(!n)return E();let J=null;if(t)try{J=n.renderSubset(t)}catch{J=n.render()}else J=n.render();for(const i of J)j(i.node_line,i.glyph),i.link_line&&G(i.link_line),j(i.pad_lines);let K=null;u&&(K=u({circles:S,r:$,updateViewbox:(e,n)=>{const[t,a]=I(e,n);z(t,a)}}));const{viewBox:U,height:Q,width:ee}=function(){const[e,n]=I(O,Y),[t,a]=I(R,Z),r=Math.min(e,t),i=Math.min(n,a),l=Math.abs(t-e),o=Math.abs(a-n);return{height:o,viewBox:r+" "+i+" "+l+" "+o,width:l}}();if(0===ee||0===Q)return E();const ne={alignItems:"center",justifyContent:"center",width:"100%",display:"flex",...a};return i.createElement("div",{className:"svgdag",style:ne},i.createElement("svg",{viewBox:U,width:Math.abs(ee)},i.createElement("g",{stroke:"var(--ifm-color-primary-darkest)",fill:"none",strokeWidth:2},B),i.createElement("g",{stroke:"var(--ifm-color-primary-darkest)",fill:"var(--ifm-color-primary)",strokeWidth:2},A),i.createElement("g",{stroke:"none",fill:"var(--ifm-color-content-inverse)"},M),K))}function B(e){let{initValue:n,showParents:t=!1}=e;const[a,r]=i.useState((()=>({parents:new Map,dag:null,subset:null})));const l=t?22:14;return i.createElement("div",{className:"drawdag row",style:{display:"flex",alignItems:"center",justifyContent:"center"}},i.createElement("div",{className:"col col--6"},i.createElement(u,{initValue:n,onDagParentsChange:function(e){let{input:n,parents:t,bindings:a}=e;r((e=>{let{dag:r,subset:i}=e;const l=new a.JsDag;try{l.addHeads(t,[]),r=l,i=function(e){let{input:n,parents:t,dag:a,bindings:r}=e,i=null;const l=n.match(/# order: (.*)/);if(l){const e=l[1].split(" ");i=new r.JsSet(e)}else if(t.size>10){const e=[...t.keys()].filter((e=>n.indexOf(e)>=0));for(i=a.sort(new r.JsSet(e));i.count()<10;){let e=i.count();if(i=i.union(a.parents(i)).union(a.children(a.heads(i))),e===i.count())break}}return i}({input:n,dag:r,parents:t,bindings:a})}catch(o){console.error(o)}return{...e,dag:r,parents:t,subset:i}}))}})),i.createElement("div",{className:"col col--6",style:{padding:"var(--ifm-alert-padding-vertical) var(--ifm-alert-padding-horizontal)"}},i.createElement(A,{dag:a.dag,subset:a.subset,drawExtra:function(e){let{dag:n}=e;return t&&n?function(e){let{circles:t,r:a,updateViewbox:r,xyt:l}=e;const o=[];for(const[s,{cx:d,cy:m}]of t){const e=n.parentNames(s);if(e.length>1||e.some((e=>{var n;return(null!=(n=t.get(e))?n:{}).cy!==m}))){const n=(e.length>1?"parents":"parent")+": "+e.join(", "),t=d,l=m+a+2;o.push(i.createElement("text",{x:t,y:l,textAnchor:"middle",alignmentBaseline:"hanging",fontSize:"0.7em",key:s},n)),r(t-50,l+10),r(t+50,l+10)}}return i.createElement("g",{fill:"var(--ifm-color-content)"},o)}:null}(a),columnWidth:l})))}var M,S,T,I;const $={},V="DrawDag",F={unversionedId:"internals/drawdag",id:"internals/drawdag",title:"DrawDag",description:"DrawDag provides an intuitive way to create commit graph for tests.",source:"@site/docs/internals/drawdag.md",sourceDirName:"internals",slug:"/internals/drawdag",permalink:"/docs/internals/drawdag",draft:!1,editUrl:"https://github.com/facebookexperimental/eden/tree/main/website/docs/internals/drawdag.md",tags:[],version:"current",frontMatter:{},sidebar:"tutorialSidebar",previous:{title:"Internals",permalink:"/docs/category/internals"},next:{title:"IndexedLog",permalink:"/docs/internals/indexedlog"}},Z={},R=[{value:"Background",id:"background",level:2},{value:"DrawDag language",id:"drawdag-language",level:2},{value:"Basic",id:"basic",level:3},{value:"Name at multiple locations",id:"name-at-multiple-locations",level:3},{value:"Range generation",id:"range-generation",level:3},{value:"Vertical layout",id:"vertical-layout",level:3},{value:"Try DrawDag",id:"try-drawdag",level:3},{value:"DrawDag in tests",id:"drawdag-in-tests",level:2},{value:"<code>.t</code> integration tests",id:"t-integration-tests",level:3},{value:"Rust unit tests",id:"rust-unit-tests",level:3}],Y={toc:R};function O(e){let{components:n,...t}=e;return(0,l.mdx)("wrapper",(0,a.Z)({},Y,t,{components:n,mdxType:"MDXLayout"}),(0,l.mdx)("h1",{id:"drawdag"},"DrawDag"),(0,l.mdx)("p",null,"DrawDag provides an intuitive way to create commit graph for tests."),(0,l.mdx)("h2",{id:"background"},"Background"),(0,l.mdx)("p",null,"When creating tests, we often need to create a repo with a particular layout.\nFor example, to create a linear graph with three commits, we could use the\nfollowing sequence of commands:"),(0,l.mdx)("pre",null,(0,l.mdx)("code",{parentName:"pre",className:"language-sl-shell-example"},"$ sl commit -m A\n$ sl commit -m B\n$ sl commit -m C\n")),(0,l.mdx)("p",null,"If the graph is nonlinear, extra commands such as merge and goto are needed:"),(0,l.mdx)("pre",null,(0,l.mdx)("code",{parentName:"pre",className:"language-sl-shell-example"},"$ sl commit -m A\n$ sl commit -m B\n$ sl goto -q '.^'\n$ sl commit -m C\n$ sl merge -q 'desc(B)'\n$ sl commit -m D\n")),(0,l.mdx)("p",null,"As you can see, creating the desired graph shape via writing out a sequence of\ncommands is tedious, potentially error prone, and not immediately obvious what\nthe resulting graph looks like."),(0,l.mdx)("p",null,"To help aid people in writing tests (and those reviewing the tests!), we've\ncreated DrawDag to simply and intuitively create repos with the desired shape."),(0,l.mdx)("h2",{id:"drawdag-language"},"DrawDag language"),(0,l.mdx)("p",null,"DrawDag is a domain specific language to describe a DAG (Directed Acyclic Graph)."),(0,l.mdx)("h3",{id:"basic"},"Basic"),(0,l.mdx)("p",null,"In this example, the DrawDag code looks like a hexagon and generates the graph\nto the right:"),(0,l.mdx)(B,{showParents:!0,initValue:String.raw(M||(M=(0,r.Z)(["\n    -B-\n   /     A--C--D\n      /\n    E-F\n"],["\n    -B-\n   /   \\\n  A--C--D\n   \\   /\n    E-F\n"]))),mdxType:"DrawDagExample"}),(0,l.mdx)("p",null,"The DrawDag code forms a 2D matrix of characters. There are three types of\ncharacters:"),(0,l.mdx)("ul",null,(0,l.mdx)("li",{parentName:"ul"},"Space characters."),(0,l.mdx)("li",{parentName:"ul"},"Connect characters: ",(0,l.mdx)("inlineCode",{parentName:"li"},"-"),",  ",(0,l.mdx)("inlineCode",{parentName:"li"},"\\"),", and ",(0,l.mdx)("inlineCode",{parentName:"li"},"/"),"."),(0,l.mdx)("li",{parentName:"ul"},"Name characters: alpha, numeric, and some other characters.")),(0,l.mdx)("p",null,"Names define vertexes in the graph. Connect characters define edges in the graph."),(0,l.mdx)("p",null,"If two vertexes are directly connected, the one to the left becomes a parent of\nthe other vertex. For a commit graph, this behaves like making commits from\nleft to right."),(0,l.mdx)("p",null,"If a vertex has multiple parents, those parents are sorted in lexicographical\norder."),(0,l.mdx)("h3",{id:"name-at-multiple-locations"},"Name at multiple locations"),(0,l.mdx)("p",null,"A single name can be used in multiple locations and will represent the same\nvertex in the graph."),(0,l.mdx)("p",null,"For example, the code below uses ",(0,l.mdx)("inlineCode",{parentName:"p"},"C")," in two locations to create criss-cross\nmerges."),(0,l.mdx)(B,{initValue:String.raw(S||(S=(0,r.Z)(["\n  A-C\n     B-D\n       C\n"],["\n  A-C\n   \\\n  B-D\n   \\\n    C\n"]))),mdxType:"DrawDagExample"}),(0,l.mdx)("h3",{id:"range-generation"},"Range generation"),(0,l.mdx)("p",null,"You can use ",(0,l.mdx)("inlineCode",{parentName:"p"},"..")," (or more dots) to generate a range of vertexes and connect\nthem. This works for simple alphabet names like ",(0,l.mdx)("inlineCode",{parentName:"p"},"A..Z")," or numbers like\n",(0,l.mdx)("inlineCode",{parentName:"p"},"A01..A99"),":"),(0,l.mdx)(B,{initValue:String.raw(T||(T=(0,r.Z)(["\n  A..C...F\n       /\n       K\n"],["\n  A..C...F\n      \\ /\n       K\n"]))),mdxType:"DrawDagExample"}),(0,l.mdx)("p",null,"The range expansion under the hood works similarly to\n",(0,l.mdx)("a",{parentName:"p",href:"https://www.ruby-lang.org/"},"Ruby"),"'s ",(0,l.mdx)("a",{parentName:"p",href:"https://ruby-doc.org/core/Range.html"},"Range"),"."),(0,l.mdx)("h3",{id:"vertical-layout"},"Vertical layout"),(0,l.mdx)("p",null,"By default, DrawDag assumes a horizontal layout. You can opt-in the alternative\nvertical layout by using ",(0,l.mdx)("inlineCode",{parentName:"p"},"|"),", or ",(0,l.mdx)("inlineCode",{parentName:"p"},":"),". It has a few differences:"),(0,l.mdx)("ul",null,(0,l.mdx)("li",{parentName:"ul"},(0,l.mdx)("inlineCode",{parentName:"li"},"|")," is a valid connect character. ",(0,l.mdx)("inlineCode",{parentName:"li"},"-")," becomes invalid."),(0,l.mdx)("li",{parentName:"ul"},(0,l.mdx)("inlineCode",{parentName:"li"},":")," is used for range generation. ",(0,l.mdx)("inlineCode",{parentName:"li"},".")," becomes a valid name character.")),(0,l.mdx)(B,{initValue:String.raw(I||(I=(0,r.Z)(["\n  Z\n  :  C B\n  |/\n  A\n"],["\n  Z\n  :\\\n  C B\n  |/\n  A\n"]))),mdxType:"DrawDagExample"}),(0,l.mdx)("p",null,"Commits are created from bottom to top. This is similar to ",(0,l.mdx)("inlineCode",{parentName:"p"},"sl log -G")," output\norder."),(0,l.mdx)("h3",{id:"try-drawdag"},"Try DrawDag"),(0,l.mdx)("p",null,"Try editing the DrawDag code above. We draw the output live in the browser."),(0,l.mdx)("h2",{id:"drawdag-in-tests"},"DrawDag in tests"),(0,l.mdx)("h3",{id:"t-integration-tests"},(0,l.mdx)("inlineCode",{parentName:"h3"},".t")," integration tests"),(0,l.mdx)("p",null,"You can use the ",(0,l.mdx)("inlineCode",{parentName:"p"},"drawdag")," shell function in ",(0,l.mdx)("inlineCode",{parentName:"p"},".t")," tests to create commits and\nchange the repo."),(0,l.mdx)("pre",null,(0,l.mdx)("code",{parentName:"pre",className:"language-sl-shell-example"},"$ drawdag << 'EOS'\n>  C\n>  |\n> B1 B2  # amend: B1 -> B2\n>   \\|\n>    A\n> EOS\n")),(0,l.mdx)("p",null,(0,l.mdx)("inlineCode",{parentName:"p"},"#")," starts a comment till the end of the line. Comments won't be parsed as\nDrawDag code but might have other meanings:"),(0,l.mdx)("ul",null,(0,l.mdx)("li",{parentName:"ul"},(0,l.mdx)("inlineCode",{parentName:"li"},"# A/dir/file = 1"),": In commit ",(0,l.mdx)("inlineCode",{parentName:"li"},"A"),", update path ",(0,l.mdx)("inlineCode",{parentName:"li"},"dir/file")," to content ",(0,l.mdx)("inlineCode",{parentName:"li"},"1"),"."),(0,l.mdx)("li",{parentName:"ul"},(0,l.mdx)("inlineCode",{parentName:"li"},"# amend: X -> Y -> Z"),": Mark ",(0,l.mdx)("inlineCode",{parentName:"li"},"Y")," as amended from ",(0,l.mdx)("inlineCode",{parentName:"li"},"X"),", ",(0,l.mdx)("inlineCode",{parentName:"li"},"Z")," as amended from ",(0,l.mdx)("inlineCode",{parentName:"li"},"Y"),"."),(0,l.mdx)("li",{parentName:"ul"},(0,l.mdx)("inlineCode",{parentName:"li"},"# bookmark FOO = A"),": Create bookmark ",(0,l.mdx)("inlineCode",{parentName:"li"},"FOO")," that points to commit ",(0,l.mdx)("inlineCode",{parentName:"li"},"A"),".")),(0,l.mdx)("p",null,"You can also use revset expressions to refer to existing commits. For example,\n",(0,l.mdx)("inlineCode",{parentName:"p"},".")," in vertical layout refers to the working parent."),(0,l.mdx)("p",null,"Check ",(0,l.mdx)("inlineCode",{parentName:"p"},"test-drawdag.t")," for more examples."),(0,l.mdx)("h3",{id:"rust-unit-tests"},"Rust unit tests"),(0,l.mdx)("p",null,"You can use the ",(0,l.mdx)("inlineCode",{parentName:"p"},"drawdag")," crate to parse DrawDag code into graph vertexes and\nedges."),(0,l.mdx)("p",null,"The ",(0,l.mdx)("inlineCode",{parentName:"p"},"dag")," crate might also be useful to run complex queries on a graph, and\nrender it as ASCII."))}O.isMDXComponent=!0},24654:()=>{}}]);