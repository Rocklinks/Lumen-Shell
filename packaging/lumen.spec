%define _unpackaged_files_terminate_build 0

Name:        lumen
Version:     0.3.0
Release:     1%{?dist}
Summary:     Lightweight shell for XFCE with GNOME Shell look and feel, built from scratch

License:     MIT
URL:         https://github.com/Rocklinks/Lumen-Shell
Source0:     %{name}-%{version}.tar.gz

BuildArch:   x86_64
# Runtime: dynamically linked binary needs only the system C library (always present)
# No other runtime dependencies. Build requires rust/cargo only if building from source.

%description
lumen is a single-binary shell designed to run on top of XFCE, delivering
a modern GNOME Shell-style interface without the weight of a full desktop
environment. Built from scratch in Rust (std-only).

It links lumen-shell, lumen-bt and lumen-connect into one lumen binary.

lumen-bt: bluetooth without bluetoothd or bluetoothctl (power, scan, pair,
connect, disconnect, forget, battery, trusted).

lumen-connect: KDE Connect v8 service addon (notifications, calls, SMS
alerts, files, clipboard sync, find my phone, battery).

%prep
%setup -q

%build
# Binary is prebuilt; nothing to compile here. To build from source:
# cargo build --release

%install
install -d %{buildroot}%{_bindir}
install -d %{buildroot}%{_datadir}/doc/%{name}
install -d %{buildroot}%{_mandir}/man1

install -m 0755 %{name} %{buildroot}%{_bindir}/%{name}
install -m 0644 README.md %{buildroot}%{_datadir}/doc/%{name}/README
install -m 0644 COPYING %{buildroot}%{_datadir}/doc/%{name}/copyright 2>/dev/null || :

%files
%{_bindir}/%{name}
%{_datadir}/doc/%{name}/*
%doc README.md

%changelog
* Mon Nov 09 2026 Rocklin K S <Rocklin2603@gmail.com> - 0.3.0-1
- Release v0.3.0
