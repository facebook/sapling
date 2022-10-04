AutoReq: no
AutoProv: no
AutoReqProv: no

%define py_version 3.8
%define py_version_num 38
%define py_usr_dir %{_prefix}
%define __python python3.8

%undefine __brp_mangle_shebangs


Summary: Sapling Beta
Name: sapling-beta
Version: %version
Release: darwin

License: GPLv2
Group: Development/Tools

Requires: python%{py_version_num}

%description
A source control system.

%prep

%build

%install
cd "%{sapling_root}"
make DESTDIR="$RPM_BUILD_ROOT" PREFIX="%{_prefix}" install-oss

# Add the Sapling binary to the PATH
mkdir -p $RPM_BUILD_ROOT%{_sysconfdir}/paths.d/
echo %{_bindir} > $RPM_BUILD_ROOT%{_sysconfdir}/paths.d/sapling

rm "$RPM_BUILD_ROOT%{_bindir}/hg"
mv "$RPM_BUILD_ROOT%{_bindir}/sl" "$RPM_BUILD_ROOT%{_bindir}/slb"

%post
umask 022
%{_bindir}/slb debugpython -c "import compileall, sys; sys.exit(not compileall.compile_dir('%{_prefix}', ddir='/', force=True, quiet=1))"

%clean
rm -rf "$RPM_BUILD_ROOT"

%files
%defattr(-,root,wheel,-)

%{_prefix}
%{_bindir}
%{_libdir}
%{_sysconfdir}/paths.d/sapling-beta

%changelog
