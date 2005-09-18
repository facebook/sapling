Summary: Mercurial -- a distributed SCM
Name: mercurial
Version: 0.7
Release: 1
License: GPL
Group: Development/Tools
Source: http://www.selenic.com/mercurial/release/%{name}-%{version}.tar.gz
URL: http://www.selenic.com/mercurial
BuildRoot: /tmp/build.%{name}-%{version}-%{release}

%define pythonver %(python -c 'import sys;print ".".join(map(str, sys.version_info[:2]))')
%define pythonlib %{_libdir}/python%{pythonver}/site-packages/%{name}

%description
Mercurial is a fast, lightweight source control management system designed
for efficient handling of very large distributed projects.

%prep
rm -rf $RPM_BUILD_ROOT
%setup -q

%build
python setup.py build

%install
python setup.py install --root $RPM_BUILD_ROOT

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc doc/* contrib/patchbomb *.cgi
%dir %{pythonlib}
%{_bindir}/hgmerge
%{_bindir}/hg
%{pythonlib}/templates
%{pythonlib}/*.py*
%{pythonlib}/*.so
