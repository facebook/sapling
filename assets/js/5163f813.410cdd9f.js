"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[7931],{3905:(e,t,n)=>{n.r(t),n.d(t,{MDXContext:()=>m,MDXProvider:()=>c,mdx:()=>f,useMDXComponents:()=>u,withMDXComponents:()=>p});var i=n(67294);function r(e,t,n){return t in e?Object.defineProperty(e,t,{value:n,enumerable:!0,configurable:!0,writable:!0}):e[t]=n,e}function o(){return o=Object.assign||function(e){for(var t=1;t<arguments.length;t++){var n=arguments[t];for(var i in n)Object.prototype.hasOwnProperty.call(n,i)&&(e[i]=n[i])}return e},o.apply(this,arguments)}function a(e,t){var n=Object.keys(e);if(Object.getOwnPropertySymbols){var i=Object.getOwnPropertySymbols(e);t&&(i=i.filter((function(t){return Object.getOwnPropertyDescriptor(e,t).enumerable}))),n.push.apply(n,i)}return n}function l(e){for(var t=1;t<arguments.length;t++){var n=null!=arguments[t]?arguments[t]:{};t%2?a(Object(n),!0).forEach((function(t){r(e,t,n[t])})):Object.getOwnPropertyDescriptors?Object.defineProperties(e,Object.getOwnPropertyDescriptors(n)):a(Object(n)).forEach((function(t){Object.defineProperty(e,t,Object.getOwnPropertyDescriptor(n,t))}))}return e}function s(e,t){if(null==e)return{};var n,i,r=function(e,t){if(null==e)return{};var n,i,r={},o=Object.keys(e);for(i=0;i<o.length;i++)n=o[i],t.indexOf(n)>=0||(r[n]=e[n]);return r}(e,t);if(Object.getOwnPropertySymbols){var o=Object.getOwnPropertySymbols(e);for(i=0;i<o.length;i++)n=o[i],t.indexOf(n)>=0||Object.prototype.propertyIsEnumerable.call(e,n)&&(r[n]=e[n])}return r}var m=i.createContext({}),p=function(e){return function(t){var n=u(t.components);return i.createElement(e,o({},t,{components:n}))}},u=function(e){var t=i.useContext(m),n=t;return e&&(n="function"==typeof e?e(t):l(l({},t),e)),n},c=function(e){var t=u(e.components);return i.createElement(m.Provider,{value:t},e.children)},d={inlineCode:"code",wrapper:function(e){var t=e.children;return i.createElement(i.Fragment,{},t)}},g=i.forwardRef((function(e,t){var n=e.components,r=e.mdxType,o=e.originalType,a=e.parentName,m=s(e,["components","mdxType","originalType","parentName"]),p=u(n),c=r,g=p["".concat(a,".").concat(c)]||p[c]||d[c]||o;return n?i.createElement(g,l(l({ref:t},m),{},{components:n})):i.createElement(g,l({ref:t},m))}));function f(e,t){var n=arguments,r=t&&t.mdxType;if("string"==typeof e||r){var o=n.length,a=new Array(o);a[0]=g;var l={};for(var s in t)hasOwnProperty.call(t,s)&&(l[s]=t[s]);l.originalType=e,l.mdxType="string"==typeof e?e:r,a[1]=l;for(var m=2;m<o;m++)a[m]=n[m];return i.createElement.apply(null,a)}return i.createElement.apply(null,n)}g.displayName="MDXCreateElement"},920:(e,t,n)=>{n.d(t,{RJ:()=>m,Xj:()=>l,bv:()=>s,mY:()=>a,nk:()=>p});var i=n(67294),r=n(44996),o=n(50941);function a(e){let{name:t,linkText:n}=e;const o=function(e){switch(e){case"go":return"goto";case"isl":return"web"}return e}(t),a=null!=n?n:t;return i.createElement("a",{href:(0,r.default)("/docs/commands/"+o)},i.createElement("code",null,a))}function l(e){let{name:t}=e;return i.createElement(a,{name:t,linkText:"sl "+t})}function s(){return i.createElement("p",{style:{textAlign:"center"}},i.createElement("img",{src:(0,r.default)("/img/reviewstack-demo.gif"),width:800,align:"center"}))}function m(e){let{alt:t,light:n,dark:a}=e;return i.createElement(o.Z,{alt:t,sources:{light:(0,r.default)(n),dark:(0,r.default)(a)}})}function p(e){let{src:t}=e;return i.createElement("video",{controls:!0},i.createElement("source",{src:(0,r.default)(t)}))}},82820:(e,t,n)=>{n.r(t),n.d(t,{assets:()=>m,contentTitle:()=>l,default:()=>c,frontMatter:()=>a,metadata:()=>s,toc:()=>p});var i=n(83117),r=(n(67294),n(3905)),o=n(920);const a={sidebar_position:4},l="Signing Commits",s={unversionedId:"git/signing",id:"git/signing",title:"Signing Commits",description:'Currently, signing is only supported with commits in Git repos. See Git\'s documentation on "Signing Your Work" for more context.',source:"@site/docs/git/signing.md",sourceDirName:"git",slug:"/git/signing",permalink:"/docs/git/signing",draft:!1,editUrl:"https://github.com/facebookexperimental/eden/tree/main/website/docs/git/signing.md",tags:[],version:"current",sidebarPosition:4,frontMatter:{sidebar_position:4},sidebar:"tutorialSidebar",previous:{title:"Sapling stack",permalink:"/docs/git/sapling-stack"},next:{title:"Submodule",permalink:"/docs/git/submodule"}},m={},p=[{value:"Limitations",id:"limitations",level:2},{value:"Troubleshooting",id:"troubleshooting",level:2}],u={toc:p};function c(e){let{components:t,...n}=e;return(0,r.mdx)("wrapper",(0,i.Z)({},u,n,{components:t,mdxType:"MDXLayout"}),(0,r.mdx)("h1",{id:"signing-commits"},"Signing Commits"),(0,r.mdx)("p",null,"Currently, signing is only supported with commits in Git repos. See ",(0,r.mdx)("a",{parentName:"p",href:"https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work"},'Git\'s documentation on "Signing Your Work" for more context'),"."),(0,r.mdx)("p",null,"Note that Sapling has a single configuration for your identity:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"$ sl config ui.username\nAlyssa P. Hacker <alyssa@example.com>\n")),(0,r.mdx)("p",null,"whereas Git has these as separate items:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"$ git config user.name\nAlyssa P. Hacker\n$ git config user.email\nalyssa@example.com\n")),(0,r.mdx)("p",null,"You must ensure that:"),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},"Your value of ",(0,r.mdx)("inlineCode",{parentName:"li"},"ui.username")," can be parsed as ",(0,r.mdx)("inlineCode",{parentName:"li"},"NAME <EMAIL>"),"."),(0,r.mdx)("li",{parentName:"ul"},"When parsed, these values match what you specified for ",(0,r.mdx)("strong",{parentName:"li"},"Real name")," and ",(0,r.mdx)("strong",{parentName:"li"},"Email address")," when you created your GPG key.")),(0,r.mdx)("p",null,"In Git, you would configure your repo for automatic signing via:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"git config --local user.signingkey B577AA76BAE505B1\ngit config --local commit.gpgsign true\n")),(0,r.mdx)("p",null,"Because Sapling does not read values from ",(0,r.mdx)("inlineCode",{parentName:"p"},"git config"),", you must add the analogous configuration to Sapling as follows:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"sl config --local gpg.key B577AA76BAE505B1\n")),(0,r.mdx)("p",null,"Sapling's equivalent to Git's ",(0,r.mdx)("inlineCode",{parentName:"p"},"commit.gpgsign")," config is ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg.enabled"),", but it\ndefaults to ",(0,r.mdx)("inlineCode",{parentName:"p"},"true"),"."),(0,r.mdx)("p",null,"Note that ",(0,r.mdx)("inlineCode",{parentName:"p"},"--local")," is used to enable signing for the ",(0,r.mdx)("em",{parentName:"p"},"current")," repository. Use ",(0,r.mdx)("inlineCode",{parentName:"p"},"--user")," to default to signing for ",(0,r.mdx)("em",{parentName:"p"},"all")," repositories on your machine."),(0,r.mdx)("h2",{id:"limitations"},"Limitations"),(0,r.mdx)("p",null,"Support for signing commits is relatively new in Sapling, so we only support a subset of Git's functionality, for now. Specifically:"),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},"There is no ",(0,r.mdx)("inlineCode",{parentName:"li"},"-S")," option for ",(0,r.mdx)(o.Xj,{name:"commit",mdxType:"SLCommand"})," or other commands, as signing is expected to be set for the repository. To disable signing for an individual action, leveraging the ",(0,r.mdx)("inlineCode",{parentName:"li"},"--config")," flag like so should work, but has not been heavily tested:")),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"sl --config gpg.enabled=false <command> <args>\n")),(0,r.mdx)("ul",null,(0,r.mdx)("li",{parentName:"ul"},"While Git supports multiple signing schemes (",(0,r.mdx)("a",{parentName:"li",href:"https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key"},"GPG, SSH, or X.509"),"), Sapling supports only GPG at this time.")),(0,r.mdx)("h2",{id:"troubleshooting"},"Troubleshooting"),(0,r.mdx)("p",null,"The Git documentation on GPG is a bit light on detail when it comes to ensuring you have GPG configured correctly."),(0,r.mdx)("p",null,"First, make sure that ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg")," is available on your ",(0,r.mdx)("inlineCode",{parentName:"p"},"$PATH")," and that ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg --list-secret-keys --keyid-format LONG")," lists the keys you expect. Note that you will have to run ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg --gen-key")," to create a key that matches your Sapling identity if you do not have one available already."),(0,r.mdx)("p",null,"A basic test to ensure that ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg")," is setup correctly is to use it to sign a pice of test data:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},'echo "test" | gpg --clearsign\n')),(0,r.mdx)("p",null,"If you see ",(0,r.mdx)("inlineCode",{parentName:"p"},"error: gpg failed to sign the data"),", try this StackOverflow article:"),(0,r.mdx)("p",null,(0,r.mdx)("a",{parentName:"p",href:"https://stackoverflow.com/questions/39494631/gpg-failed-to-sign-the-data-fatal-failed-to-write-commit-object-git-2-10-0"},"https://stackoverflow.com/questions/39494631/gpg-failed-to-sign-the-data-fatal-failed-to-write-commit-object-git-2-10-0")),(0,r.mdx)("p",null,"If you see ",(0,r.mdx)("inlineCode",{parentName:"p"},"gpg: signing failed: Inappropriate ioctl for device"),", try:"),(0,r.mdx)("pre",null,(0,r.mdx)("code",{parentName:"pre"},"export GPG_TTY=$(tty)\n")))}c.isMDXComponent=!0}}]);