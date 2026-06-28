DOCKER_IMAGE_NAME ?= portiq
PROFILE ?=
FILTER ?=
TAG ?= latest

.PHONY: all help \
	check format lint \
	build test clean \
	docker-build

all: help

help:
	@echo "Available targets:"
	@echo "  check"
	@echo "  format"
	@echo "  lint"
	@echo "  build          PROFILE=release"
	@echo "  test           FILTER=my_test"
	@echo "  clean"
	@echo "  docker-build   TAG=latest"

check:
	cargo check

format:
	cargo fmt

lint:
	cargo clippy

build:
ifeq ($(PROFILE),)
	cargo build
else ifeq ($(PROFILE),release)
	cargo build --release
else
	$(error Invalid PROFILE '$(PROFILE)'. Expected 'release' or empty)
endif

test:
	cargo test $(FILTER)

clean:
	cargo clean

docker-build:
	docker build -t $(DOCKER_IMAGE_NAME):$(TAG) .
