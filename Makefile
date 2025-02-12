ARTIFACTS ?= sk-ctrl sk-driver sk-tracer
EXTRA_BUILD_ARTIFACTS ?=  skctl

TARGET=x86_64-unknown-linux-gnu
PLATFORM=linux/amd64
ifdef TARGET
  TARGETOPT=--target $(TARGET)
endif
ifdef PLATFORM
  PLATFORMOPT=--platform $(PLATFORM)
endif

COVERAGE_DIR=$(BUILD_DIR)/coverage
CARGO_HOME_ENV=CARGO_HOME=$(BUILD_DIR)/cargo PKG_CONFIG_SYSROOT_DIR=/ 

ifdef IN_CI
CARGO_TEST_PREFIX=$(CARGO_HOME_ENV) CARGO_INCREMENTAL=0 RUSTFLAGS='-Cinstrument-coverage' LLVM_PROFILE_FILE='$(BUILD_DIR)/coverage/cargo-test-%p-%m.profraw'
RUST_COVER_TYPE ?= lcov
DOCKER_ARGS=
else
RUST_COVER_TYPE=markdown
DOCKER_ARGS=-it $(PLATFORMOPT)
endif

RUST_COVER_FILE=$(COVERAGE_DIR)/rust-coverage.$(RUST_COVER_TYPE)

include build/base.mk
include build/k8s.mk

RUST_BUILD_IMAGE ?= rust:buster

# This is sorta subtle; the three "main" artifacts get built inside docker containers
# to ensure that they are built against the right libs that they'll be running on in
# the cluster.  So for those we share CARGO_HOME_ENV, which needs to be in $(BUILD_DIR)
# so we have a known location for it.  This is _not_ built in a docker container so that
# because it's designed to run on the user's machine, so we don't use the custom CARGO_HOME_ENV
$(EXTRA_BUILD_ARTIFACTS)::
	PKG_CONFIG_SYSROOT_DIR=/ cargo build --target-dir=$(BUILD_DIR) --bin=$@ --color=always
	cp $(BUILD_DIR)/debug/$@ $(BUILD_DIR)/.

$(ARTIFACTS)::
	docker run $(DOCKER_ARGS) -u `id -u`:`id -g` -w /build -v `pwd`:/build:ro -v $(BUILD_DIR):/build/.build:rw $(RUST_BUILD_IMAGE) make $@-docker

%-docker:
	$(CARGO_HOME_ENV) cargo build  $(TARGETOPT) --target-dir=$(BUILD_DIR) --bin=$* --color=always
	cp $(BUILD_DIR)/$(TARGET)/debug/$* $(BUILD_DIR)/.

lint:
	cargo +nightly fmt
	cargo clippy

test: test-unit test-int

.PHONY: test-unit
test-unit:
	mkdir -p $(BUILD_DIR)/coverage
	rm -f $(BUILD_DIR)/coverage/*.profraw
	$(CARGO_TEST_PREFIX) cargo test --features=testutils $(CARGO_TEST) $(patsubst %, --bin %, $(ARTIFACTS) $(EXTRA_BUILD_ARTIFACTS)) --lib -- --nocapture --skip itest

.PHONY: test-int
test-int:
	$(CARGO_TEST_PREFIX) cargo test --features=testutils itest --lib -- --nocapture

cover:
	grcov . --binary-path $(BUILD_DIR)/debug/deps -s . -t $(RUST_COVER_TYPE) -o $(RUST_COVER_FILE) --branch \
		--ignore '../*' \
		--ignore '/*' \
		--ignore '*/tests/*' \
		--ignore '*_test.rs' \
		--ignore 'src/api/v1/*' \
		--ignore 'src/metrics/api/*' \
		--ignore 'src/testutils/*' \
		--ignore '.build/*' \
		--excl-line '#\[derive' \
		--excl-start '#\[cfg\((test|feature = "testutils")'
	@if [ "$(RUST_COVER_TYPE)" = "markdown" ]; then cat $(RUST_COVER_FILE); fi

.PHONY: crd
crd: skctl
	$(BUILD_DIR)/skctl crd > k8s/raw/simkube.io_simulations.yml

.PHONY: api
api:
	openapi-generator generate -i api/v1/simkube.yml -g rust --global-property models -o generated-api
	cp generated-api/src/models/export_filters.rs lib/rust/api/v1/.
	cp generated-api/src/models/export_request.rs lib/rust/api/v1/.
	@echo ''
	@echo '----------------------------------------------------------------------'
	@echo 'WARNING: YOU NEED TO DO MANUAL CLEANUP TO THE OPENAPI GENERATED FILES!'
	@echo '----------------------------------------------------------------------'
	@echo 'At a minimum:'
	@echo '   In lib/rust/api/v1/*, add "use super::*", and replace all the'
	@echo '   k8s-generated types with the correct imports from k8s-openapi'
	@echo '----------------------------------------------------------------------'
	@echo 'CHECK THE DIFF CAREFULLY!!!'
	@echo '----------------------------------------------------------------------'
	@echo ''
	@echo 'Eventually we would like to automate more of this, but it does not'
	@echo 'happen right now.  :('
	@echo ''
