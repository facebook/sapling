%global emacs_lispdir %{_datadir}/emacs/site-lisp

%define withpython %{nil}

%if "%{?withpython}"

%global pythonver %{withpython}
%global pythonname Python-%{withpython}
%global docutilsname docutils-0.12
%global docutilsmd5 4622263b62c5c771c03502afa3157768
%global pythonhg python-hg
%global hgpyprefix /opt/%{pythonhg}
# byte compilation will fail on some some Python /test/ files
%global _python_bytecompile_errors_terminate_build 0

%else

%global pythonver %(python -c 'import sys;print ".".join(map(str, sys.version_info[:2]))')

%endif

Summary: A fast, lightweight Source Control Management system
Name: mercurial
Version: snapshot
Release: 0
License: GPLv2+
Group: Development/Tools
URL: https://mercurial-scm.org/
Source0: %{name}-%{version}-%{release}.tar.gz
%if "%{?withpython}"
Source1: %{pythonname}.tgz
Source2: %{docutilsname}.tar.gz
%endif
BuildRoot: %{_tmppath}/%{name}-%{version}-%{release}-root

BuildRequires: make, gcc, gettext
%if "%{?withpython}"
BuildRequires: readline-devel, openssl-devel, ncurses-devel, zlib-devel, bzip2-devel
%else
BuildRequires: python >= 2.6, python-devel, python-docutils >= 0.5
Requires: python >= 2.6
%endif
# The hgk extension uses the wish tcl interpreter, but we don't enforce it
#Requires: tk

%description
Mercurial is a fast, lightweight source control management system designed
for efficient handling of very large distributed projects.

%prep

%if "%{?withpython}"
%setup -q -n mercurial-%{version}-%{release} -a1 -a2
# despite the comments in cgi.py, we do this to prevent rpmdeps from picking /usr/local/bin/python up
sed -i '1c#! /usr/bin/env python' %{pythonname}/Lib/cgi.py
%else
%setup -q -n mercurial-%{version}-%{release}
%endif

%build

%if "%{?withpython}"

PYPATH=$PWD/%{pythonname}
cd $PYPATH
./configure --prefix=%{hgpyprefix}
make all %{?_smp_mflags}
cd -

cd %{docutilsname}
LD_LIBRARY_PATH=$PYPATH $PYPATH/python setup.py build
cd -

# verify Python environment
LD_LIBRARY_PATH=$PYPATH PYTHONPATH=$PWD/%{docutilsname} $PYPATH/python -c 'import sys, zlib, bz2, ssl, curses, readline'

# set environment for make
export PATH=$PYPATH:$PATH
export LD_LIBRARY_PATH=$PYPATH
export CFLAGS="-L $PYPATH"
export PYTHONPATH=$PWD/%{docutilsname}

%endif

make all

%install
rm -rf $RPM_BUILD_ROOT

%if "%{?withpython}"

PYPATH=$PWD/%{pythonname}
cd $PYPATH
make install DESTDIR=$RPM_BUILD_ROOT
# these .a are not necessary and they are readonly and strip fails - kill them!
rm -f %{buildroot}%{hgpyprefix}/lib/{,python2.*/config}/libpython2.*.a
cd -

cd %{docutilsname}
LD_LIBRARY_PATH=$PYPATH $PYPATH/python setup.py install --root="$RPM_BUILD_ROOT"
cd -

PATH=$PYPATH:$PATH LD_LIBRARY_PATH=$PYPATH make install DESTDIR=$RPM_BUILD_ROOT PREFIX=%{hgpyprefix} MANDIR=%{_mandir}
mkdir -p $RPM_BUILD_ROOT%{_bindir}
( cd $RPM_BUILD_ROOT%{_bindir}/ && ln -s ../..%{hgpyprefix}/bin/hg . )
( cd $RPM_BUILD_ROOT%{_bindir}/ && ln -s ../..%{hgpyprefix}/bin/python2.? %{pythonhg} )

%else

make install DESTDIR=$RPM_BUILD_ROOT PREFIX=%{_prefix} MANDIR=%{_mandir}

%endif

install -m 755 contrib/hgk $RPM_BUILD_ROOT%{_bindir}/
install -m 755 contrib/hg-ssh $RPM_BUILD_ROOT%{_bindir}/

bash_completion_dir=$RPM_BUILD_ROOT%{_sysconfdir}/bash_completion.d
mkdir -p $bash_completion_dir
install -m 644 contrib/bash_completion $bash_completion_dir/mercurial.sh

zsh_completion_dir=$RPM_BUILD_ROOT%{_datadir}/zsh/site-functions
mkdir -p $zsh_completion_dir
install -m 644 contrib/zsh_completion $zsh_completion_dir/_mercurial

mkdir -p $RPM_BUILD_ROOT%{emacs_lispdir}
install -m 644 contrib/mercurial.el $RPM_BUILD_ROOT%{emacs_lispdir}/
install -m 644 contrib/mq.el $RPM_BUILD_ROOT%{emacs_lispdir}/

mkdir -p $RPM_BUILD_ROOT/%{_sysconfdir}/mercurial/hgrc.d

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc CONTRIBUTORS COPYING doc/README doc/hg*.txt doc/hg*.html *.cgi contrib/*.fcgi
%doc %attr(644,root,root) %{_mandir}/man?/hg*
%doc %attr(644,root,root) contrib/*.svg
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
%if "%{?withpython}"
%{_bindir}/%{pythonhg}
%{hgpyprefix}
%else
%if "%{?pythonver}" != "2.4"
%{_libdir}/python%{pythonver}/site-packages/%{name}-*-py%{pythonver}.egg-info
%endif
%{_libdir}/python%{pythonver}/site-packages/%{name}
%{_libdir}/python%{pythonver}/site-packages/hgext
%{_libdir}/python%{pythonver}/site-packages/hgext3rd
%endif
