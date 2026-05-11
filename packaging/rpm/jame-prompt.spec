Name:           jame-prompt
Version:        1.1.0
Release:        1%{?dist}
Summary:        JamePrompt lightweight local prompt manager

License:        MIT
URL:            https://github.com/roymejia2217/JamePrompt
Source0:        %{name}-%{version}.tar.gz

# Fedora's automatic debugsource generation can produce an empty
# debugsourcefiles.list for this Rust GUI package in containerized builds.
%global debug_package %{nil}

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  desktop-file-utils
BuildRequires:  pkgconfig(gtk+-3.0)
BuildRequires:  pkgconfig(x11)
BuildRequires:  pkgconfig(xtst)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(fontconfig)
BuildRequires:  pkgconfig(freetype2)
BuildRequires:  pkgconfig(gdk-pixbuf-2.0)

Requires:       gtk3
Requires:       libxdo
Requires:       libX11
Requires:       libXtst
Requires:       libxkbcommon
Requires:       fontconfig
Requires:       freetype
Requires:       gdk-pixbuf2
Requires:       hicolor-icon-theme

%description
JamePrompt is a lightweight local prompt manager with SQLite storage, global
hotkeys, clipboard integration, paste simulation, Linux system tray support,
and autostart integration.

%prep
%autosetup

%build
if [ "${JAME_PROMPT_REUSE_RELEASE_BUILD:-0}" != "1" ]; then
  cargo build --release --locked
fi

%check
cargo test --locked --bin %{name}
desktop-file-validate packaging/linux/jame-prompt.desktop

%install
install -Dm755 target/release/%{name} %{buildroot}%{_bindir}/%{name}
desktop-file-install \
  --dir=%{buildroot}%{_datadir}/applications \
  packaging/linux/jame-prompt.desktop
install -Dm644 packaging/linux/jame-prompt.1 %{buildroot}%{_mandir}/man1/%{name}.1
install -Dm644 LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE
install -Dm644 packaging/linux/copyright %{buildroot}%{_docdir}/%{name}/copyright

for size in 16 22 24 32 48 64 128 256 512; do
  install -Dm644 assets/icons/app_icon.png \
    %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/%{name}.png
done

%files
%license %{_licensedir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/copyright
%{_bindir}/jame-prompt
%{_datadir}/applications/jame-prompt.desktop
%{_datadir}/icons/hicolor/*/apps/jame-prompt.png
%{_mandir}/man1/jame-prompt.1*

%changelog
* Mon May 11 2026 Roy Mejia <roymejia2217@gmail.com> - 1.1.0-1
- Add JSON prompt import and export from settings.
- Add empty-state guidance for new prompt libraries.

* Sat May 02 2026 Roy Mejia <roymejia2217@gmail.com> - 1.0.0-1
- Initial Fedora and RHEL RPM packaging recipe.
