"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[9606],{3905:(e,n,t)=>{t.r(n),t.d(n,{MDXContext:()=>r,MDXProvider:()=>p,mdx:()=>f,useMDXComponents:()=>u,withMDXComponents:()=>s});var o=t(67294);function a(e,n,t){return n in e?Object.defineProperty(e,n,{value:t,enumerable:!0,configurable:!0,writable:!0}):e[n]=t,e}function l(){return l=Object.assign||function(e){for(var n=1;n<arguments.length;n++){var t=arguments[n];for(var o in t)Object.prototype.hasOwnProperty.call(t,o)&&(e[o]=t[o])}return e},l.apply(this,arguments)}function i(e,n){var t=Object.keys(e);if(Object.getOwnPropertySymbols){var o=Object.getOwnPropertySymbols(e);n&&(o=o.filter((function(n){return Object.getOwnPropertyDescriptor(e,n).enumerable}))),t.push.apply(t,o)}return t}function m(e){for(var n=1;n<arguments.length;n++){var t=null!=arguments[n]?arguments[n]:{};n%2?i(Object(t),!0).forEach((function(n){a(e,n,t[n])})):Object.getOwnPropertyDescriptors?Object.defineProperties(e,Object.getOwnPropertyDescriptors(t)):i(Object(t)).forEach((function(n){Object.defineProperty(e,n,Object.getOwnPropertyDescriptor(t,n))}))}return e}function d(e,n){if(null==e)return{};var t,o,a=function(e,n){if(null==e)return{};var t,o,a={},l=Object.keys(e);for(o=0;o<l.length;o++)t=l[o],n.indexOf(t)>=0||(a[t]=e[t]);return a}(e,n);if(Object.getOwnPropertySymbols){var l=Object.getOwnPropertySymbols(e);for(o=0;o<l.length;o++)t=l[o],n.indexOf(t)>=0||Object.prototype.propertyIsEnumerable.call(e,t)&&(a[t]=e[t])}return a}var r=o.createContext({}),s=function(e){return function(n){var t=u(n.components);return o.createElement(e,l({},n,{components:t}))}},u=function(e){var n=o.useContext(r),t=n;return e&&(t="function"==typeof e?e(n):m(m({},n),e)),t},p=function(e){var n=u(e.components);return o.createElement(r.Provider,{value:n},e.children)},c={inlineCode:"code",wrapper:function(e){var n=e.children;return o.createElement(o.Fragment,{},n)}},h=o.forwardRef((function(e,n){var t=e.components,a=e.mdxType,l=e.originalType,i=e.parentName,r=d(e,["components","mdxType","originalType","parentName"]),s=u(t),p=a,h=s["".concat(i,".").concat(p)]||s[p]||c[p]||l;return t?o.createElement(h,m(m({ref:n},r),{},{components:t})):o.createElement(h,m({ref:n},r))}));function f(e,n){var t=arguments,a=n&&n.mdxType;if("string"==typeof e||a){var l=t.length,i=new Array(l);i[0]=h;var m={};for(var d in n)hasOwnProperty.call(n,d)&&(m[d]=n[d]);m.originalType=e,m.mdxType="string"==typeof e?e:a,i[1]=m;for(var r=2;r<l;r++)i[r]=t[r];return o.createElement.apply(null,i)}return o.createElement.apply(null,t)}h.displayName="MDXCreateElement"},93090:(e,n,t)=>{t.r(n),t.d(n,{assets:()=>d,contentTitle:()=>i,default:()=>u,frontMatter:()=>l,metadata:()=>m,toc:()=>r});var o=t(83117),a=(t(67294),t(3905));const l={},i="Submodule",m={unversionedId:"git/submodule",id:"git/submodule",title:"Submodule",description:"Sapling has basic support for Git submodules.",source:"@site/docs/git/submodule.md",sourceDirName:"git",slug:"/git/submodule",permalink:"/docs/git/submodule",draft:!1,editUrl:"https://github.com/facebookexperimental/eden/tree/main/website/docs/git/submodule.md",tags:[],version:"current",frontMatter:{},sidebar:"tutorialSidebar",previous:{title:"Signing Commits",permalink:"/docs/git/signing"},next:{title:"Commands",permalink:"/docs/category/commands"}},d={},r=[{value:"Concepts",id:"concepts",level:2},{value:"Git submodule",id:"git-submodule",level:3},{value:"Submodule as a single file",id:"submodule-as-a-single-file",level:3},{value:"Submodule as a repository",id:"submodule-as-a-repository",level:3},{value:"Common operations",id:"common-operations",level:2},{value:"Clone a repository with submodules",id:"clone-a-repository-with-submodules",level:3},{value:"Use a different commit in a submodule",id:"use-a-different-commit-in-a-submodule",level:3},{value:"Show changed files in a submodule",id:"show-changed-files-in-a-submodule",level:3},{value:"Pull submodule changes",id:"pull-submodule-changes",level:3},{value:"Push submodule changes",id:"push-submodule-changes",level:3},{value:"Add, remove, or rename a submodule",id:"add-remove-or-rename-a-submodule",level:3}],s={toc:r};function u(e){let{components:n,...t}=e;return(0,a.mdx)("wrapper",(0,o.Z)({},s,t,{components:n,mdxType:"MDXLayout"}),(0,a.mdx)("h1",{id:"submodule"},"Submodule"),(0,a.mdx)("p",null,"Sapling has basic support for Git submodules."),(0,a.mdx)("p",null,"Sapling does not have a ",(0,a.mdx)("inlineCode",{parentName:"p"},"submodule")," command. Commands that change the working\ncopy like ",(0,a.mdx)("inlineCode",{parentName:"p"},"goto")," or ",(0,a.mdx)("inlineCode",{parentName:"p"},"clone")," will recursively change submodules. Other commands\nlike ",(0,a.mdx)("inlineCode",{parentName:"p"},"commit"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"pull"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"status"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"diff")," will treat a submodule as a special\nfile that only contains a commit hash. Those commands ignore files inside\nsubmodules."),(0,a.mdx)("h2",{id:"concepts"},"Concepts"),(0,a.mdx)("h3",{id:"git-submodule"},"Git submodule"),(0,a.mdx)("p",null,"A Git submodule has three basic properties: URL (where to fetch the submodule),\npath (where to write to), and commit hash (which commit to use)."),(0,a.mdx)("p",null,"The URL and path are specified in the check-in file ",(0,a.mdx)("inlineCode",{parentName:"p"},".gitmodules"),". The commit\nhash is stored specially at the given path."),(0,a.mdx)("p",null,"Depending on operations, a submodule might behave like a file or a repository."),(0,a.mdx)("h3",{id:"submodule-as-a-single-file"},"Submodule as a single file"),(0,a.mdx)("p",null,"When you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"diff"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"cat"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"status")," or commands that directly or indirectly\nask for the content of a submodule, the submodule behaves like a single file\nwith the content ",(0,a.mdx)("inlineCode",{parentName:"p"},"Subproject commit HASH"),", it will not behave like a directory."),(0,a.mdx)("p",null,"For example, ",(0,a.mdx)("inlineCode",{parentName:"p"},"status")," and ",(0,a.mdx)("inlineCode",{parentName:"p"},"diff")," only shows the commit hash change of\nsubmodules. They do not show individual file changes inside the submodules.\n",(0,a.mdx)("inlineCode",{parentName:"p"},"sl cat")," treats file paths inside submodules as non existent."),(0,a.mdx)("p",null,"When you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"commit"),", a submodule is also treated as a single file with just\nits commit hash. ",(0,a.mdx)("inlineCode",{parentName:"p"},"commit")," will not recursively make commits in submodules.\nSame for ",(0,a.mdx)("inlineCode",{parentName:"p"},"amend"),"."),(0,a.mdx)("h3",{id:"submodule-as-a-repository"},"Submodule as a repository"),(0,a.mdx)("p",null,"When you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"goto"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"revert")," or commands that ask Sapling to change the\nworking copy to match the content of a submodule, Sapling will pull the\nsubmodule on demand, create the submodule repository on demand, and ask the\nsubmodule repository to checkout the specified commit."),(0,a.mdx)("p",null,"When you use ",(0,a.mdx)("inlineCode",{parentName:"p"},"cd")," to enter a submodule, the submodule works like a standalone\nrepository."),(0,a.mdx)("h2",{id:"common-operations"},"Common operations"),(0,a.mdx)("h3",{id:"clone-a-repository-with-submodules"},"Clone a repository with submodules"),(0,a.mdx)("p",null,"Sapling clones submodules recursively ",(0,a.mdx)("sup",{parentName:"p",id:"fnref-1"},(0,a.mdx)("a",{parentName:"sup",href:"#fn-1",className:"footnote-ref"},"1")),"; there is no need to use flags like\n",(0,a.mdx)("inlineCode",{parentName:"p"},"--recurse"),", or use additional commands to initialize the submodules."),(0,a.mdx)("h3",{id:"use-a-different-commit-in-a-submodule"},"Use a different commit in a submodule"),(0,a.mdx)("p",null,"Imagine you have a submodule at ",(0,a.mdx)("inlineCode",{parentName:"p"},"third_party/fmt"),". The submodule is currently\nat commit ",(0,a.mdx)("inlineCode",{parentName:"p"},"a337011"),", and you want to use commit ",(0,a.mdx)("inlineCode",{parentName:"p"},"1f575fd")," instead. You can make\nsuch change by running ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl goto")," in the submodule:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ cd third_party/fmt\n$ sl goto 1f575fd\n")),(0,a.mdx)("p",null,"Now the parent repo will notice the change. ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl status")," will show\n",(0,a.mdx)("inlineCode",{parentName:"p"},"third_party/fmt"),' as "modified":'),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ cd ../..\n$ sl status\nM third_party/fmt\n")),(0,a.mdx)("p",null,"You can run ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl diff")," to double check the commit hash change is from\n",(0,a.mdx)("inlineCode",{parentName:"p"},"a337011")," to ",(0,a.mdx)("inlineCode",{parentName:"p"},"1f575fd"),":"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ sl diff\ndiff --git a/third_party/fmt b/third_party/fmt\n--- a/third_party/fmt\n+++ b/third_party/fmt\n@@ -1,1 +1,1 @@\n-Subproject commit a33701196adfad74917046096bf5a2aa0ab0bb50\n+Subproject commit 1f575fd5c90278bcf723f72737f0f63c1951bea3\n")),(0,a.mdx)("p",null,"If you need to abandon changes in a submodule, use ",(0,a.mdx)("inlineCode",{parentName:"p"},"revert"),":"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ sl revert third_party/fmt\n")),(0,a.mdx)("p",null,"Finally, remember to commit the submodule change:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},'$ sl commit -m "Update third_party/fmt to 1f575fd"\n')),(0,a.mdx)("p",null,"Note ",(0,a.mdx)("inlineCode",{parentName:"p"},"commit")," only makes a single commit in the parent repo. It does not\nrecursively make commits in submodules. This is because the parent repo only\ntracks the commit hashes of submodules and does not directly care about\nchanged files in submodules."),(0,a.mdx)("h3",{id:"show-changed-files-in-a-submodule"},"Show changed files in a submodule"),(0,a.mdx)("p",null,"You can use ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl status")," within a submodule to list changed files in that\nsubmodule:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ cd third_party/fmt\n$ sl status\n")),(0,a.mdx)("p",null,"Running ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl status")," from the parent repo will not list changed files in\nsubmodule. Although changed files are not shown, changed commits are\nalways shown. You might want to always ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl commit")," changes in submodules\nso submodule changes can be detected from the parent repo when using\n",(0,a.mdx)("inlineCode",{parentName:"p"},"sl status")," or ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl diff"),"."),(0,a.mdx)("p",null,"If you do need to list changed files in all submodules, you might want to\nuse a shell script like:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre",className:"language-bash"},"for i in `grep 'path =' .gitmodules | sed 's/.*=//'`; do sl status --pager=off --cwd $i; done\n")),(0,a.mdx)("p",null,"In the future we might add a convenient way to run ",(0,a.mdx)("inlineCode",{parentName:"p"},"status")," recursively in\nsubmodules."),(0,a.mdx)("h3",{id:"pull-submodule-changes"},"Pull submodule changes"),(0,a.mdx)("p",null,"When you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl goto")," from the parent repo, Sapling will pull required\nsubmodule repos on demand in order to complete the ",(0,a.mdx)("inlineCode",{parentName:"p"},"goto")," operation."),(0,a.mdx)("p",null,"Right now, Sapling might only pull the commit needed and will not pull branches\nlike ",(0,a.mdx)("inlineCode",{parentName:"p"},"main")," or ",(0,a.mdx)("inlineCode",{parentName:"p"},"master"),". If you want to pull branches explicitly, you can pull\nit within the submodule:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ cd path/to/submodule\n$ sl pull -B main\n")),(0,a.mdx)("p",null,"If you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl pull")," from the parent repo, Sapling does not pull submodule\nrepos recursively."),(0,a.mdx)("h3",{id:"push-submodule-changes"},"Push submodule changes"),(0,a.mdx)("p",null,"You can push submodule changes to the remote server by running ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl push")," within\nthe submodule:"),(0,a.mdx)("pre",null,(0,a.mdx)("code",{parentName:"pre"},"$ cd path/to/submodule\n$ sl push --to main\n")),(0,a.mdx)("p",null,"If you run ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl push")," from the parent repo, Sapling does not push submodule\nrepos recursively."),(0,a.mdx)("h3",{id:"add-remove-or-rename-a-submodule"},"Add, remove, or rename a submodule"),(0,a.mdx)("p",null,"Right now, these are not supported. In the future we might make ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl clone"),"\ndetect the submodule use-case, and write the repo data to the right location,\nand update ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl add"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl mv"),", ",(0,a.mdx)("inlineCode",{parentName:"p"},"sl rm")," to update ",(0,a.mdx)("inlineCode",{parentName:"p"},".gitmodules")," automatically."),(0,a.mdx)("div",{className:"footnotes"},(0,a.mdx)("hr",{parentName:"div"}),(0,a.mdx)("ol",{parentName:"div"},(0,a.mdx)("li",{parentName:"ol",id:"fn-1"},"Submodules are not cloned like regular repos where there is usually a\n",(0,a.mdx)("inlineCode",{parentName:"li"},"remote/main")," branch after clone. This is because Sapling attempts to pull\nby the commit hash to complete the working copy update. To obtain\n",(0,a.mdx)("inlineCode",{parentName:"li"},"remote/main")," in a submodule, you can run ",(0,a.mdx)("inlineCode",{parentName:"li"},"sl pull -B main"),".",(0,a.mdx)("a",{parentName:"li",href:"#fnref-1",className:"footnote-backref"},"\u21a9")))))}u.isMDXComponent=!0}}]);