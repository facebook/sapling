Summary: Mercurial -- a distributed SCM
Name: mercurial
Version: snapshot
Release: 0
License: GPLv2
Group: Development/Tools
URL: http://mercurial.selenic.com/
Source0: http://mercurial.selenic.com/release/%{name}-%{version}.tar.gz
BuildRoot: %{_tmppath}/%{name}-%{version}-%{release}-root

# From the README:
#
#   Note: some distributions fails to include bits of distutils by
#   default, you'll need python-dev to install. You'll also need a C
#   compiler and a 3-way merge tool like merge, tkdiff, or kdiff3.
#
# python-devel provides an adequate python-dev.  The merge tool is a
# run-time dependency.
#
BuildRequires: python >= 2.4, python-devel, make, gcc, asciidoc, xmlto
Provides: hg = %{version}-%{release}

%define pythonver %(python -c 'import sys;print ".".join(map(str, sys.version_info[:2]))')
%define emacs_lispdir %{_datadir}/emacs/site-lisp

%description
Mercurial is a fast, lightweight source control management system designed
for efficient handling of very large distributed projects.

%prep
%setup -q

%build
make all

%install
rm -rf $RPM_BUILD_ROOT
python setup.py install --root $RPM_BUILD_ROOT --prefix %{_prefix}
make install-doc DESTDIR=$RPM_BUILD_ROOT MANDIR=%{_mandir}

install contrib/hgk          $RPM_BUILD_ROOT%{_bindir}
install contrib/convert-repo $RPM_BUILD_ROOT%{_bindir}/mercurial-convert-repo
install contrib/hg-ssh       $RPM_BUILD_ROOT%{_bindir}
install contrib/git-viz/{hg-viz,git-rev-tree} $RPM_BUILD_ROOT%{_bindir}

bash_completion_dir=$RPM_BUILD_ROOT%{_sysconfdir}/bash_completion.d
mkdir -p $bash_completion_dir
install -m 644 contrib/bash_completion $bash_completion_dir/mercurial.sh

zsh_completion_dir=$RPM_BUILD_ROOT%{_datadir}/zsh/site-functions
mkdir -p $zsh_completion_dir
install -m 644 contrib/zsh_completion $zsh_completion_dir/_mercurial

mkdir -p $RPM_BUILD_ROOT%{emacs_lispdir}
install contrib/mercurial.el $RPM_BUILD_ROOT%{emacs_lispdir}

mkdir -p $RPM_BUILD_ROOT/%{_sysconfdir}/mercurial/hgrc.d
install contrib/mergetools.hgrc $RPM_BUILD_ROOT%{_sysconfdir}/mercurial/hgrc.d/mergetools.rc

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc CONTRIBUTORS COPYING doc/README doc/hg*.txt doc/hg*.html doc/ja *.cgi contrib/*.fcgi
%doc %attr(644,root,root) %{_mandir}/man?/hg*.gz
%doc %attr(644,root,root) contrib/*.svg contrib/sample.hgrc
%{_sysconfdir}/bash_completion.d/mercurial.sh
%{_datadir}/zsh/site-functions/_mercurial
%{_datadir}/emacs/site-lisp/mercurial.el
%{_bindir}/hg
%{_bindir}/hgk
%{_bindir}/hg-ssh
%{_bindir}/hg-viz
%{_bindir}/git-rev-tree
%{_bindir}/mercurial-convert-repo
%dir %{_sysconfdir}/bash_completion.d/
%dir %{_datadir}/zsh/site-functions/
%dir %{_sysconfdir}/mercurial
%dir %{_sysconfdir}/mercurial/hgrc.d
%config(noreplace) %{_sysconfdir}/mercurial/hgrc.d/mergetools.rc
%if "%{?pythonver}" != "2.4"
%{_libdir}/python%{pythonver}/site-packages/%{name}-*-py%{pythonver}.egg-info
%endif
%{_libdir}/python%{pythonver}/site-packages/%{name}
%{_libdir}/python%{pythonver}/site-packages/hgext
