# Distribution-independent build and install. No administrator access is needed with a user PREFIX.
#
#   make                         build release binaries
#   make install                 install into /usr/local   (needs write access, e.g. sudo)
#   make install PREFIX=~/.local install for the current user
#   make uninstall PREFIX=...    remove it again
PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

bindir = $(DESTDIR)$(PREFIX)/bin
appdir = $(DESTDIR)$(PREFIX)/share/applications
icondir = $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps
docdir = $(DESTDIR)$(PREFIX)/share/doc/facet
licdir = $(DESTDIR)$(PREFIX)/share/licenses/facet

.PHONY: build install uninstall test clean

build:
	$(CARGO) build --release --locked -p facet-cli -p facet-app

install: build
	install -Dm755 target/release/facet     $(bindir)/facet
	install -Dm755 target/release/facet-app $(bindir)/facet-app
	# The desktop entry points at the installed binary so it works from any prefix.
	install -d $(appdir)
	sed 's|^Exec=facet-app|Exec=$(PREFIX)/bin/facet-app|' packaging/dev.facet.Facet.desktop > $(appdir)/dev.facet.Facet.desktop
	install -Dm644 assets/brand/facet.svg $(icondir)/dev.facet.Facet.svg
	install -Dm644 README.md CHANGELOG.md docs/report-schema.md -t $(docdir)
	install -Dm644 LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md -t $(licdir)
	install -Dm644 assets/fonts/IBMPlexSans-LICENSE.txt $(licdir)/IBMPlexSans-OFL.txt
	install -Dm644 assets/fonts/Lilex-OFL.txt $(licdir)/Lilex-OFL.txt
	install -Dm644 assets/icons/LICENSE $(licdir)/Lucide-ISC.txt
	-update-desktop-database $(appdir) 2>/dev/null
	-gtk-update-icon-cache -q -t $(DESTDIR)$(PREFIX)/share/icons/hicolor 2>/dev/null

uninstall:
	rm -f $(bindir)/facet $(bindir)/facet-app $(appdir)/dev.facet.Facet.desktop $(icondir)/dev.facet.Facet.svg
	rm -rf $(docdir) $(licdir)

test:
	$(CARGO) test --workspace --locked

clean:
	$(CARGO) clean
