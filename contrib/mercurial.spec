Summary: Mercurial -- a distributed SCM
Name: mercurial
Version: 0.6
Release: 1
Copyright: GPL
Group: Development/Tools
Distribution: RedHat
Source: http://www.selenic.com/mercurial/release/%{name}-%{version}.tar.gz
Packager: Arun Sharma <arun@sharma-home.net>
Prefix: /usr
BuildRoot: /tmp/build.%{name}-%{version}-%{release}

%define pythonver %(python -c 'import sys;print ".".join(map(str, sys.version_info[:2]))')
%define pythonlib %{_libdir}/python%{pythonver}/site-packages/%{name}

%description

Mercurial is a fast, lightweight source control management system designed
for efficient handling of very large distributed projects.

%prep

rm -rf $RPM_BUILD_ROOT

%setup -q -n %{name}-%{version}

%build

python setup.py build

%install

python setup.py install --root $RPM_BUILD_ROOT

%clean
rm -rf $RPM_BUILD_ROOT

%files
%defattr(-,root,root,-)
%doc doc
%dir %{pythonlib}
%{_bindir}/hgmerge
%{_bindir}/hg
%{pythonlib}/templates
%{pythonlib}/*.pyc
%{pythonlib}/*.py
%{pythonlib}/*.so
