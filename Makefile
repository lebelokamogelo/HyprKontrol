PREFIX  ?= /usr/local
DESTDIR ?=

BIN   = $(DESTDIR)$(PREFIX)/bin/hyprkontrol
ENTRY = $(DESTDIR)$(PREFIX)/share/applications/hyprkontrol.desktop
ICON  = $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/hyprkontrol.svg

.PHONY: build check install uninstall

build:
	cargo build --release --locked

check:
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo test --locked

# Does not build, so that `sudo make install` never runs cargo as root.
install:
	install -Dm755 target/release/hyprkontrol $(BIN)
	install -Dm644 packaging/hyprkontrol.desktop $(ENTRY)
	install -Dm644 packaging/hyprkontrol.svg $(ICON)

uninstall:
	rm -f $(BIN) $(ENTRY) $(ICON)
