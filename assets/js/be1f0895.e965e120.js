"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[8221],{3905:(e,t,n)=>{n.r(t),n.d(t,{MDXContext:()=>o,MDXProvider:()=>s,mdx:()=>f,useMDXComponents:()=>c,withMDXComponents:()=>p});var a=n(67294);function r(e,t,n){return t in e?Object.defineProperty(e,t,{value:n,enumerable:!0,configurable:!0,writable:!0}):e[t]=n,e}function i(){return i=Object.assign||function(e){for(var t=1;t<arguments.length;t++){var n=arguments[t];for(var a in n)Object.prototype.hasOwnProperty.call(n,a)&&(e[a]=n[a])}return e},i.apply(this,arguments)}function m(e,t){var n=Object.keys(e);if(Object.getOwnPropertySymbols){var a=Object.getOwnPropertySymbols(e);t&&(a=a.filter((function(t){return Object.getOwnPropertyDescriptor(e,t).enumerable}))),n.push.apply(n,a)}return n}function d(e){for(var t=1;t<arguments.length;t++){var n=null!=arguments[t]?arguments[t]:{};t%2?m(Object(n),!0).forEach((function(t){r(e,t,n[t])})):Object.getOwnPropertyDescriptors?Object.defineProperties(e,Object.getOwnPropertyDescriptors(n)):m(Object(n)).forEach((function(t){Object.defineProperty(e,t,Object.getOwnPropertyDescriptor(n,t))}))}return e}function l(e,t){if(null==e)return{};var n,a,r=function(e,t){if(null==e)return{};var n,a,r={},i=Object.keys(e);for(a=0;a<i.length;a++)n=i[a],t.indexOf(n)>=0||(r[n]=e[n]);return r}(e,t);if(Object.getOwnPropertySymbols){var i=Object.getOwnPropertySymbols(e);for(a=0;a<i.length;a++)n=i[a],t.indexOf(n)>=0||Object.prototype.propertyIsEnumerable.call(e,n)&&(r[n]=e[n])}return r}var o=a.createContext({}),p=function(e){return function(t){var n=c(t.components);return a.createElement(e,i({},t,{components:n}))}},c=function(e){var t=a.useContext(o),n=t;return e&&(n="function"==typeof e?e(t):d(d({},t),e)),n},s=function(e){var t=c(e.components);return a.createElement(o.Provider,{value:t},e.children)},u={inlineCode:"code",wrapper:function(e){var t=e.children;return a.createElement(a.Fragment,{},t)}},x=a.forwardRef((function(e,t){var n=e.components,r=e.mdxType,i=e.originalType,m=e.parentName,o=l(e,["components","mdxType","originalType","parentName"]),p=c(n),s=r,x=p["".concat(m,".").concat(s)]||p[s]||u[s]||i;return n?a.createElement(x,d(d({ref:t},o),{},{components:n})):a.createElement(x,d({ref:t},o))}));function f(e,t){var n=arguments,r=t&&t.mdxType;if("string"==typeof e||r){var i=n.length,m=new Array(i);m[0]=x;var d={};for(var l in t)hasOwnProperty.call(t,l)&&(d[l]=t[l]);d.originalType=e,d.mdxType="string"==typeof e?e:r,m[1]=d;for(var o=2;o<i;o++)m[o]=n[o];return a.createElement.apply(null,m)}return a.createElement.apply(null,n)}x.displayName="MDXCreateElement"},85058:(e,t,n)=>{n.r(t),n.d(t,{assets:()=>l,contentTitle:()=>m,default:()=>c,frontMatter:()=>i,metadata:()=>d,toc:()=>o});var a=n(83117),r=(n(67294),n(3905));const i={sidebar_position:20},m=void 0,d={unversionedId:"commands/histedit",id:"commands/histedit",title:"histedit",description:"histedit",source:"@site/docs/commands/histedit.md",sourceDirName:"commands",slug:"/commands/histedit",permalink:"/docs/commands/histedit",draft:!1,editUrl:"https://github.com/facebookexperimental/eden/tree/main/website/docs/commands/histedit.md",tags:[],version:"current",sidebarPosition:20,frontMatter:{sidebar_position:20},sidebar:"tutorialSidebar",previous:{title:"hide",permalink:"/docs/commands/hide"},next:{title:"init",permalink:"/docs/commands/init"}},l={},o=[{value:"histedit",id:"histedit",level:2},{value:"arguments",id:"arguments",level:2}],p={toc:o};function c(e){let{components:t,...n}=e;return(0,r.mdx)("wrapper",(0,a.Z)({},p,n,{components:t,mdxType:"MDXLayout"}),(0,r.mdx)("h2",{id:"histedit"},"histedit"),(0,r.mdx)("p",null,(0,r.mdx)("strong",{parentName:"p"},"interactively reorder, combine, or delete commits")),(0,r.mdx)("p",null,"This command lets you edit a linear series of commits up to\nand including the working copy, which should be clean.\nYou can:"),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"pick")," to (re)order a commit")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"drop")," to omit a commit")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"mess")," to reword a commit message")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"fold")," to combine a commit with the preceding commit, using the later date")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"roll")," like fold, but discarding this commit's description and date")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"edit")," to edit a commit, preserving date")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},(0,r.mdx)("inlineCode",{parentName:"p"},"base")," to checkout a commit and continue applying subsequent commits"))),(0,r.mdx)("p",null,"There are multiple ways to select the root changeset:"),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},"Specify ANCESTOR directly")),(0,r.mdx)("li",{parentName:"ul"},(0,r.mdx)("p",{parentName:"li"},"Otherwise, the value from the ",(0,r.mdx)("inlineCode",{parentName:"p"},"histedit.defaultrev")," config option  is used as a revset to select the base commit when ANCESTOR is not  specified. The first commit returned by the revset is used. By  default, this selects the editable history that is unique to the  ancestry of the working directory."))),(0,r.mdx)("p",null,"Examples:"),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},"A number of changes have been made.  Commit ",(0,r.mdx)("inlineCode",{parentName:"li"},"a113a4006")," is no longer needed.")),(0,r.mdx)("p",null,"Start history editing from commit a:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"sl histedit -r a113a4006\n")),(0,r.mdx)("p",null,"An editor opens, containing the list of commits,\nwith specific actions specified:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"pick a113a4006 Zworgle the foobar\npick 822478b68 Bedazzle the zerlog\npick d275e7ed9 5 Morgify the cromulancy\n")),(0,r.mdx)("p",null,"Additional information about the possible actions\nto take appears below the list of commits."),(0,r.mdx)("p",null,"To remove commit ",(0,r.mdx)("a",{parentName:"p",href:"https://github.com/facebook/sapling/commit/a113a4006"},(0,r.mdx)("inlineCode",{parentName:"a"},"a113a40"))," from the history,\nits action (at the beginning of the relevant line)\nis changed to ",(0,r.mdx)("inlineCode",{parentName:"p"},"drop"),":"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"drop a113a4006 Zworgle the foobar\npick 822478b68 Bedazzle the zerlog\npick d275e7ed9 Morgify the cromulancy\n")),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},"A number of changes have been made.  Commit ",(0,r.mdx)("a",{parentName:"li",href:"https://github.com/facebook/sapling/commit/fe2bff2ce"},(0,r.mdx)("inlineCode",{parentName:"a"},"fe2bff2"))," and ",(0,r.mdx)("a",{parentName:"li",href:"https://github.com/facebook/sapling/commit/c9116c09e"},(0,r.mdx)("inlineCode",{parentName:"a"},"c9116c0"))," need to be swapped.")),(0,r.mdx)("p",null,"Start history editing from commit ",(0,r.mdx)("a",{parentName:"p",href:"https://github.com/facebook/sapling/commit/fe2bff2ce"},(0,r.mdx)("inlineCode",{parentName:"a"},"fe2bff2")),":"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"sl histedit -r fe2bff2ce\n")),(0,r.mdx)("p",null,"An editor opens, containing the list of commits,\nwith specific actions specified:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"pick fe2bff2ce Blorb a morgwazzle\npick 99a93da65 Zworgle the foobar\npick c9116c09e Bedazzle the zerlog\n")),(0,r.mdx)("p",null,"To swap commits ",(0,r.mdx)("a",{parentName:"p",href:"https://github.com/facebook/sapling/commit/fe2bff2ce"},(0,r.mdx)("inlineCode",{parentName:"a"},"fe2bff2"))," and ",(0,r.mdx)("a",{parentName:"p",href:"https://github.com/facebook/sapling/commit/c9116c09e"},(0,r.mdx)("inlineCode",{parentName:"a"},"c9116c0")),", simply swap their lines:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"pick 8ef592ce7cc4 4 Bedazzle the zerlog\npick 5339bf82f0ca 3 Zworgle the foobar\npick 252a1af424ad 2 Blorb a morgwazzle\n")),(0,r.mdx)("p",null,"Returns 0 on success, 1 if user intervention is required for\n",(0,r.mdx)("inlineCode",{parentName:"p"},"edit")," command or to resolve merge conflicts."),(0,r.mdx)("h2",{id:"arguments"},"arguments"),(0,r.mdx)("table",null,(0,r.mdx)("thead",{parentName:"table"},(0,r.mdx)("tr",{parentName:"thead"},(0,r.mdx)("th",{parentName:"tr",align:null},"shortname"),(0,r.mdx)("th",{parentName:"tr",align:null},"fullname"),(0,r.mdx)("th",{parentName:"tr",align:null},"default"),(0,r.mdx)("th",{parentName:"tr",align:null},"description"))),(0,r.mdx)("tbody",{parentName:"table"},(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--commands")),(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},"read history edits from the specified file")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"-c")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--continue")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"continue an edit already in progress")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--edit-plan")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"edit remaining actions list")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"-k")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--keep")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"don","'","t strip old nodes after edit is complete")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--abort")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"abort an edit in progress")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"-r")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--rev")),(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},"first revision to be edited")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"-x")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--retry")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"retry exec command that failed and try to continue")),(0,r.mdx)("tr",{parentName:"tbody"},(0,r.mdx)("td",{parentName:"tr",align:null}),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"--show-plan")),(0,r.mdx)("td",{parentName:"tr",align:null},(0,r.mdx)("inlineCode",{parentName:"td"},"false")),(0,r.mdx)("td",{parentName:"tr",align:null},"show remaining actions list")))))}c.isMDXComponent=!0}}]);