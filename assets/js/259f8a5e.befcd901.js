"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[3323],{3905:(e,r,n)=>{n.r(r),n.d(r,{MDXContext:()=>c,MDXProvider:()=>p,mdx:()=>h,useMDXComponents:()=>d,withMDXComponents:()=>m});var t=n(67294);function i(e,r,n){return r in e?Object.defineProperty(e,r,{value:n,enumerable:!0,configurable:!0,writable:!0}):e[r]=n,e}function a(){return a=Object.assign||function(e){for(var r=1;r<arguments.length;r++){var n=arguments[r];for(var t in n)Object.prototype.hasOwnProperty.call(n,t)&&(e[t]=n[t])}return e},a.apply(this,arguments)}function o(e,r){var n=Object.keys(e);if(Object.getOwnPropertySymbols){var t=Object.getOwnPropertySymbols(e);r&&(t=t.filter((function(r){return Object.getOwnPropertyDescriptor(e,r).enumerable}))),n.push.apply(n,t)}return n}function s(e){for(var r=1;r<arguments.length;r++){var n=null!=arguments[r]?arguments[r]:{};r%2?o(Object(n),!0).forEach((function(r){i(e,r,n[r])})):Object.getOwnPropertyDescriptors?Object.defineProperties(e,Object.getOwnPropertyDescriptors(n)):o(Object(n)).forEach((function(r){Object.defineProperty(e,r,Object.getOwnPropertyDescriptor(n,r))}))}return e}function l(e,r){if(null==e)return{};var n,t,i=function(e,r){if(null==e)return{};var n,t,i={},a=Object.keys(e);for(t=0;t<a.length;t++)n=a[t],r.indexOf(n)>=0||(i[n]=e[n]);return i}(e,r);if(Object.getOwnPropertySymbols){var a=Object.getOwnPropertySymbols(e);for(t=0;t<a.length;t++)n=a[t],r.indexOf(n)>=0||Object.prototype.propertyIsEnumerable.call(e,n)&&(i[n]=e[n])}return i}var c=t.createContext({}),m=function(e){return function(r){var n=d(r.components);return t.createElement(e,a({},r,{components:n}))}},d=function(e){var r=t.useContext(c),n=r;return e&&(n="function"==typeof e?e(r):s(s({},r),e)),n},p=function(e){var r=d(e.components);return t.createElement(c.Provider,{value:r},e.children)},u={inlineCode:"code",wrapper:function(e){var r=e.children;return t.createElement(t.Fragment,{},r)}},f=t.forwardRef((function(e,r){var n=e.components,i=e.mdxType,a=e.originalType,o=e.parentName,c=l(e,["components","mdxType","originalType","parentName"]),m=d(n),p=i,f=m["".concat(o,".").concat(p)]||m[p]||u[p]||a;return n?t.createElement(f,s(s({ref:r},c),{},{components:n})):t.createElement(f,s({ref:r},c))}));function h(e,r){var n=arguments,i=r&&r.mdxType;if("string"==typeof e||i){var a=n.length,o=new Array(a);o[0]=f;var s={};for(var l in r)hasOwnProperty.call(r,l)&&(s[l]=r[l]);s.originalType=e,s.mdxType="string"==typeof e?e:i,o[1]=s;for(var c=2;c<a;c++)o[c]=n[c];return t.createElement.apply(null,o)}return t.createElement.apply(null,n)}f.displayName="MDXCreateElement"},58961:(e,r,n)=>{n.r(r),n.d(r,{assets:()=>l,contentTitle:()=>o,default:()=>d,frontMatter:()=>a,metadata:()=>s,toc:()=>c});var t=n(83117),i=(n(67294),n(3905));const a={sidebar_position:50},o="Differences from Mercurial",s={unversionedId:"introduction/differences-hg",id:"introduction/differences-hg",title:"Differences from Mercurial",description:"While Sapling began 10 years ago as a variant of Mercurial, it has evolved into its own source control system and has many incompatible differences with Mercurial.",source:"@site/docs/introduction/differences-hg.md",sourceDirName:"introduction",slug:"/introduction/differences-hg",permalink:"/docs/introduction/differences-hg",draft:!1,editUrl:"https://github.com/facebookexperimental/eden/tree/main/website/docs/introduction/differences-hg.md",tags:[],version:"current",sidebarPosition:50,frontMatter:{sidebar_position:50},sidebar:"tutorialSidebar",previous:{title:"Differences from Git",permalink:"/docs/introduction/differences-git"},next:{title:"Debugging Sapling SCM",permalink:"/docs/introduction/debugging"}},l={},c=[{value:"Sapling has different default behavior and options for many commands.",id:"sapling-has-different-default-behavior-and-options-for-many-commands",level:4},{value:"Sapling has no \u201cnamed branches\u201d.",id:"sapling-has-no-named-branches",level:4},{value:"Sapling has remote bookmarks.",id:"sapling-has-remote-bookmarks",level:4},{value:"Sapling allows hiding/unhiding commits.",id:"sapling-allows-hidingunhiding-commits",level:4},{value:"Sapling makes editing commits a first-class operation.",id:"sapling-makes-editing-commits-a-first-class-operation",level:4},{value:"Sapling supports the same revset and template features as Mercurial.",id:"sapling-supports-the-same-revset-and-template-features-as-mercurial",level:4}],m={toc:c};function d(e){let{components:r,...n}=e;return(0,i.mdx)("wrapper",(0,t.Z)({},m,n,{components:r,mdxType:"MDXLayout"}),(0,i.mdx)("h1",{id:"differences-from-mercurial"},"Differences from Mercurial"),(0,i.mdx)("p",null,"While Sapling began 10 years ago as a variant of Mercurial, it has evolved into its own source control system and has many incompatible differences with Mercurial."),(0,i.mdx)("p",null,"The list of differences below is not comprehensive, nor is it meant to be a competitive comparison of Mercurial and Sapling. It just highlights some interesting differences for curious people who are already familiar with Mercurial. Many of the differences from Git also apply to Mercurial and ",(0,i.mdx)("a",{parentName:"p",href:"/docs/introduction/differences-git"},"that list")," should be referred to as well. Sapling has substantial scaling, implementation, and format differences as well that are not covered here."),(0,i.mdx)("h4",{id:"sapling-has-different-default-behavior-and-options-for-many-commands"},"Sapling has different default behavior and options for many commands."),(0,i.mdx)("p",null,"Sapling removes or changes the behavior of a number of Mercurial commands in order to make the behavior more consistent with modern expectations. For instance, in Sapling \u2018sl log\u2019 by default shows the history from your current commit. In Mercurial ",(0,i.mdx)("inlineCode",{parentName:"p"},"hg log")," shows the history of the entire repository at once."),(0,i.mdx)("p",null,"Features that are off by default in Mercurial, like rebase, are enabled by default in Sapling."),(0,i.mdx)("h4",{id:"sapling-has-no-named-branches"},"Sapling has no \u201cnamed branches\u201d."),(0,i.mdx)("p",null,"In Mercurial, a user may create bookmarks or branches."),(0,i.mdx)("p",null,"In Sapling, there are only bookmarks.  \u201cNamed Branches\u201d in the Mercurial sense do not exist."),(0,i.mdx)("h4",{id:"sapling-has-remote-bookmarks"},"Sapling has remote bookmarks."),(0,i.mdx)("p",null,"In Mercurial, there are only local bookmarks which are synchronized with the server during push and pull."),(0,i.mdx)("p",null,"In Sapling, there are local bookmarks which only ever exist locally, and there are remote bookmarks, such as remote/main, which are immutable local representations of the location of the server bookmark at the time of the last ",(0,i.mdx)("inlineCode",{parentName:"p"},"sl pull"),"."),(0,i.mdx)("h4",{id:"sapling-allows-hidingunhiding-commits"},"Sapling allows hiding/unhiding commits."),(0,i.mdx)("p",null,"In Mercurial, to remove a commit you must either strip the commit entirely, or use an extension like \u201cEvolve\u201d to semi-permanently prune the commit."),(0,i.mdx)("p",null,"In Sapling, commits are never removed/stripped from your repository and can easily be hidden/unhidden at will."),(0,i.mdx)("h4",{id:"sapling-makes-editing-commits-a-first-class-operation"},"Sapling makes editing commits a first-class operation."),(0,i.mdx)("p",null,"The original Mercurial design avoided editing commits.  While later extensions added some ability to edit commits (rebase, amend, strip, etc), it can still feel like a second-class feature."),(0,i.mdx)("p",null,"Sapling treats editing commits as a first-class concept and provides a variety of commands for manipulating commits and recovering from manipulation mistakes."),(0,i.mdx)("h1",{id:"similarities-to-mercurial"},"Similarities to Mercurial"),(0,i.mdx)("h4",{id:"sapling-supports-the-same-revset-and-template-features-as-mercurial"},"Sapling supports the same revset and template features as Mercurial."),(0,i.mdx)("p",null,"Revsets and templates largely work the same as they do in Mercurial."))}d.isMDXComponent=!0}}]);