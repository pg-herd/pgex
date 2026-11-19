Name:           pgex
Version:        %{package_version}
Release:        %{package_release}
Summary:        Refresh PostgreSQL tables with selected data
License:        MIT
URL:            https://github.com/pg-herd/pgex
Requires:       ca-certificates

# Package the verified release executable without stripping or rewriting it.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none

%description
Copy whole tables or selected rows using SQL conditions, reusable YAML
configuration, and parallel transfers with a consistent source snapshot.

%install
install -Dm755 %{_sourcedir}/pgex %{buildroot}%{_bindir}/pgex
install -Dm644 %{_sourcedir}/README.md %{buildroot}%{_docdir}/pgex/README.md
install -Dm644 %{_sourcedir}/LICENSE %{buildroot}%{_licensedir}/pgex/LICENSE
mkdir -p %{buildroot}%{_licensedir}/pgex/third-party
cp -R %{_sourcedir}/licenses/. %{buildroot}%{_licensedir}/pgex/third-party/

%files
%{_bindir}/pgex
%doc %{_docdir}/pgex/README.md
%license %{_licensedir}/pgex/
