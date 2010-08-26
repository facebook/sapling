Summary: A fast, lightweight Source Control Management system
Name: mercurial
Version: snapshot
Release: 0
License: GPLv2+
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
BuildRequires: python >= 2.4, python-devel, make, gcc, python-docutils >= 0.5, gettext
Provides: hg = %{version}-%{release}
Requires: python >= 2.4
# The hgk extension uses the wish tcl interpreter, but we don't enforce it
#Requires: tk

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
make install DESTDIR=$RPM_BUILD_ROOT PREFIX=%{_prefix} MANDIR=%{_mandir}

install -m 755 contrib/hgk $RPM_BUILD_ROOT%{_bindir}
install -m 755 contrib/hg-ssh $RPM_BUILD_ROOT%{_bindir}

bash_completion_dir=$RPM_BUILD_ROOT%{_sysconfdir}/bash_completion.d
mkdir -p $bash_completion_dir
install -m 644 contrib/bash_completion $bash_completion_dir/mercurial.sh

zsh_completion_dir=$RPM_BUILD_ROOT%{_datadir}/zsh/site-functions
mkdir -p $zsh_completion_dir
install -m 644 contrib/zsh_completion $zsh_completion_dir/_mercurial

mkdir -p $RPM_BUILD_ROOT%{emacs_lispdir}
install -m 644 contrib/mercurial.el $RPM_BUILD_ROOT%{emacs_lispdir}
install -m 644 contrib/mq.el $RPM_BUILD_ROOT%{emacs_lispdir}

mkdir -p $RPM_BUILD_ROOT/%{_sysconfdir}/mercurial/hgrc.d
install -m 644 contrib/mergetools.hgrc $RPM_BUILD_ROOT%{_sysconfdir}/mercurial/hgrc.d/mergetools.rc

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc CONTRIBUTORS COPYING doc/README doc/hg*.txt doc/hg*.html *.cgi contrib/*.fcgi
%doc %attr(644,root,root) %{_mandir}/man?/hg*
%doc %attr(644,root,root) contrib/*.svg contrib/sample.hgrc
%dir %{_datadir}/zsh/
%dir %{_datadir}/zsh/site-functions/
%{_datadir}/zsh/site-functions/_mercurial
%dir %{_datadir}/emacs/site-lisp/
%{_datadir}/emacs/site-lisp/mercurial.el
%{_datadir}/emacs/site-lisp/mq.el
%{_bindir}/hg
%{_bindir}/hgk
%{_bindir}/hg-ssh
%dir %{_sysconfdir}/bash_completion.d/
%config(noreplace) %{_sysconfdir}/bash_completion.d/mercurial.sh
%dir %{_sysconfdir}/mercurial
%dir %{_sysconfdir}/mercurial/hgrc.d
%config(noreplace) %{_sysconfdir}/mercurial/hgrc.d/mergetools.rc
%if "%{?pythonver}" != "2.4"
%{_libdir}/python%{pythonver}/site-packages/%{name}-*-py%{pythonver}.egg-info
%endif
%{_libdir}/python%{pythonver}/site-packages/%{name}
%{_libdir}/python%{pythonver}/site-packages/hgext
