FEATURES ?= "lua51"

.PHONY: build
build: fmt
	cargo build --features ${FEATURES}


.PHONY: check
check:
	cargo clippy --features ${FEATURES} -- --no-deps -D warnings
	cargo test --tests --features ${FEATURES} -- --show-output --test-threads=1


.PHONY: bench
bench:
	cargo bench --features ${FEATURES}


.PHONY: fmt
fmt:
	cargo fmt --all


.PHONY: clean
clean:
	cargo clean


.PHONY: outdated
outdated:
	cargo install --locked cargo-outdated
	cargo outdated -R
