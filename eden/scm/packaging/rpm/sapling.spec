%define py_version 3.8
%define py_version_num 38
%define py_usr_dir /usr
%define __python python3.8
%if 0%{?fedora}
%define os_release fc29
%else
%define os_release el
%endif

%undefine __brp_mangle_shebangs

Summary: Sapling
Name: sapling
Version: %version
Release: %{os_release}

License: GPLv2
Group: Development/Tools

Requires: python%{py_version}

%description
A source control system.

%prep

%build

%install
cd "%{sapling_root}"
make DESTDIR="$RPM_BUILD_ROOT" PREFIX="%{_prefix}" install-oss

%post

%clean
rm -rf "$RPM_BUILD_ROOT"

%files
%defattr(-,root,root,-)

%{_bindir}/*
%{_libdir}/*

%changelog
