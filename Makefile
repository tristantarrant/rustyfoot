.PHONY: all build lib rust clean

all: build

build: lib rust

lib:
	$(MAKE) -C utils
	mkdir -p lib
	cp utils/libmod_utils.so lib/

rust: lib
	LD_LIBRARY_PATH=./lib cargo build

release: lib
	LD_LIBRARY_PATH=./lib cargo build --release

cross-pi4:
	cargo build --release --target aarch64-unknown-linux-gnu

run: build
	LD_LIBRARY_PATH=./lib cargo run

clean:
	$(MAKE) -C utils clean
	cargo clean
	rm -rf lib/
