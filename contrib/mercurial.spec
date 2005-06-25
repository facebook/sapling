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

cd $RPM_BUILD_ROOT
find . -type d | sed '1,2d;s,^\.,\%attr(-\,root\,root) \%dir ,' > \
	$RPM_BUILD_DIR/file.list.%{name}

find . -type f | sed -e 's,^\.,\%attr(-\,root\,root) ,' \
	-e '/\/config\//s|^|%config|' \
	-e '/\/applnk\//s|^|%config|' >> \
	$RPM_BUILD_DIR/file.list.%{name}

find . -type l | sed 's,^\.,\%attr(-\,root\,root) ,' >> \
	$RPM_BUILD_DIR/file.list.%{name}

%clean
rm -rf $RPM_BUILD_ROOT $RPM_BUILD_DIR/file.list.%{name}

%files -f ../file.list.%{name}
%doc doc
