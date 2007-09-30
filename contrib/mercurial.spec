Summary: Mercurial -- a distributed SCM
Name: mercurial
Version: snapshot
Release: 0
License: GPL
Group: Development/Tools
Source: http://www.selenic.com/mercurial/release/%{name}-%{version}.tar.gz
URL: http://www.selenic.com/mercurial
BuildRoot: /tmp/build.%{name}-%{version}-%{release}

# From the README:
#
#   Note: some distributions fails to include bits of distutils by
#   default, you'll need python-dev to install. You'll also need a C
#   compiler and a 3-way merge tool like merge, tkdiff, or kdiff3.
#
# python-devel provides an adequate python-dev.  The merge tool is a
# run-time dependency.
#
BuildRequires: python >= 2.3, python-devel, make, gcc, asciidoc, xmlto

%define pythonver %(python -c 'import sys;print ".".join(map(str, sys.version_info[:2]))')
%define pythonlib %{_libdir}/python%{pythonver}/site-packages/%{name}
%define hgext %{_libdir}/python%{pythonver}/site-packages/hgext

%description
Mercurial is a fast, lightweight source control management system designed
for efficient handling of very large distributed projects.

%prep
rm -rf $RPM_BUILD_ROOT
%setup -q

%build
make all

%install
python setup.py install --root $RPM_BUILD_ROOT --prefix %{_prefix}
make install-doc DESTDIR=$RPM_BUILD_ROOT MANDIR=%{_mandir}

install contrib/hgk          $RPM_BUILD_ROOT%{_bindir}
install contrib/convert-repo $RPM_BUILD_ROOT%{_bindir}/mercurial-convert-repo
install contrib/hg-ssh       $RPM_BUILD_ROOT%{_bindir}
install contrib/git-viz/{hg-viz,git-rev-tree} $RPM_BUILD_ROOT%{_bindir}

bash_completion_dir=$RPM_BUILD_ROOT%{_sysconfdir}/bash_completion.d
mkdir -p $bash_completion_dir
install contrib/bash_completion $bash_completion_dir/mercurial.sh

zsh_completion_dir=$RPM_BUILD_ROOT%{_datadir}/zsh/site-functions
mkdir -p $zsh_completion_dir
install contrib/zsh_completion $zsh_completion_dir/_mercurial

lisp_dir=$RPM_BUILD_ROOT%{_datadir}/emacs/site-lisp
mkdir -p $lisp_dir
install contrib/mercurial.el $lisp_dir

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc CONTRIBUTORS COPYING doc/README doc/hg*.txt doc/hg*.html doc/ja *.cgi
%{_mandir}/man?/hg*.gz
%dir %{pythonlib}
%dir %{hgext}
%{_sysconfdir}/bash_completion.d/mercurial.sh
%{_datadir}/zsh/site-functions/_mercurial
%{_datadir}/emacs/site-lisp/mercurial.el
%{_bindir}/hg
%{_bindir}/hgk
%{_bindir}/hgmerge
%{_bindir}/hg-ssh
%{_bindir}/hg-viz
%{_bindir}/git-rev-tree
%{_bindir}/mercurial-convert-repo
%{pythonlib}/templates
%{pythonlib}/*.py*
%{pythonlib}/hgweb/*.py*
%{pythonlib}/*.so
%{hgext}/*.py*
%{hgext}/convert/*.py*
