DOCKER_IMAGE_NAME := "portiq"

default:
    @just --list

check:
    cargo check

format-check:
    cargo fmt --check

format:
    cargo fmt

lint:
    cargo clippy

build profile="":
    cargo build {{profile}}

test filter="":
    cargo test {{filter}}

run *args:
    cargo run -- {{args}}

clean:
    cargo clean

doc:
    cargo doc

docker-build tag="latest":
    docker build -t {{DOCKER_IMAGE_NAME}}:{{tag}} .
