Summary: Mercurial -- a distributed SCM
Name: mercurial
Version: snapshot
Release: 0
License: GPL
Group: Development/Tools
Source: http://www.selenic.com/mercurial/release/%{name}-%{version}.tar.gz
URL: http://www.selenic.com/mercurial
BuildRoot: /tmp/build.%{name}-%{version}-%{release}

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
python setup.py build

%install
python setup.py install --root $RPM_BUILD_ROOT --prefix %{_prefix}

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc doc/* *.cgi
%dir %{pythonlib}
%dir %{hgext}
%{_bindir}/hgmerge
%{_bindir}/hg
%{pythonlib}/templates
%{pythonlib}/*.py*
%{pythonlib}/hgweb/*.py*
%{pythonlib}/*.so
%{hgext}/*.py*
