# PortIQ: A Simple HTTP(S) API Gateway in Rust

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**PortIQ** is an HTTP(S) API gateway written in Rust, built using `hyper`, `reqwest`, `rustls`, and `tokio`.

> **Note:** This project is built primarily for learning purposes to explore Rust for network programming.

## Features

- **Multiple Listeners**: Support for multiple HTTP/HTTPS/TCP listeners.
- **Virtual Hosts**: Route requests based on hostnames with SNI support.
- **Path-based Routing**: Route requests to different upstream services based on the URL path.
- **TLS Termination**: Offload TLS encryption/decryption from your backend services.
- **Load Balancing**: In-memory Weighted Round Robin (WRR) for distributing traffic.
- **Middlewares**: A configurable middleware chain for transforming request/response (design inspired by
  `reqwest-middleware` crate). Currently, a few middlewares are implemented -
    - Request ID for tracking
    - Access logger for detailed request logs
    - Request prefix to rewrite url before sending upstream
    - Token Bucket in memory rate limiter
- **Yaml Configuration**: Configure everything through a single `portiq.yml` file.
- **Config Validation**: Basic validation for configuration, such as ensuring one default TLS certificate, no duplicate
  listeners, no undefined services, etc.
- **Dynamic Config Reload**: Dynamic reload via REST API /api/v1/reload. Currently only the section under http and tcp
  can be reloaded (middlewares, services, routes). Once the file is updated hit /api/v1/reload endpoint to signal
  reload. Maybe a file watcher can also be added later.
- **Logging**: Structured and configurable logging for better monitoring.
- **API Server**: A minimal REST API to allow dynamic updates to configuration. Currently, it's very minimal just the
  below endpoints:
    - **GET /api/v1**: Returns currently applied config and some metadata.
    - **POST /api/v1/reload**: Signal server to re-read the config file and apply the config without restart.

## Getting Started

### Prerequisites

- **Rust Toolchain**: The Minimum Supported Rust Version (MSRV) is `1.79` or later. This is determined by the MSRV of
  its core dependencies (`tokio`, `hyper`, `rustls`).
- **Build Tools**: `rustls` uses `aws-lc-rs` as the default cryptography provider, which requires a C compiler (like GCC
  or Clang) and `cmake` to build. Please see the `aws-lc-rs` documentation for more details.

### 1. Build from Source

```bash
# Clone the repository
git clone https://github.com/ajju10/portiq.git
cd portiq

# Build in release mode
cargo build --release
```

### 2. Configure PortIQ

Create a `portiq.yml` file. Here's a sample configuration, most of the fields can be omitted and default values will be
picked:

```yaml
version: 1 # currently only allowed value is `1`, can be omitted

# Exposes a minimal admin API, can be omitted
admin_api:
  addr: 127.0.0.1:5678 # default

log:
  level: INFO # (could be anything supported by `tracing`) default INFO
  format: compact # (compact or json) default compact
  file_path: stdout # (could be either stdout or a file path) default stdout

# format and file_path have same options as log
access_log:
  enabled: true # (default true)
  format: compact
  file_path: stdout

tls: # List of certificates to use, only one must be marked as default, can be omitted if running http only
  - cert_file: cert.pem
    key_file: key.pem
    default: true
    hostnames: [ api.example.com ] # valid hostname matching the certificate

listeners: # One or more listeners
  - name: http-main
    addr: 0.0.0.0:3000
    protocol: http # default

  - name: https-main
    addr: 0.0.0.0:3443
    protocol: https

  - name: tcp-main
    addr: 0.0.0.0:5000
    protocol: tcp # for raw TCP listeners

http:
  middlewares: # List of named middlewares can be omitted if not required
    global-rate-limit:
      rate_limit:
        limit: 2
        period: "10s"

  services:
    user-service:
      upstreams:
        - target: https://user.service1:4443
          weight: 2 # can be omitted, default is 1
        - target: https://user.service2:5443

    internal-service:
      upstreams:
        - target: http://localhost:8000

  routes: # At least one of hosts and path is required
    - hosts: [ api.example.com ]
      path: /api/v1/*
      listeners: [ https-main ]
      service: user-service
      middlewares: [ global-rate-limit ] # middlewares can be attached to http routes

    - path: /api/internal
      listeners: [ http-main ]
      service: internal-service

tcp:
  services:
    postgres:
      upstreams:
        - target: host.postgres.com # can be multiple for load balancing

  routes:
    - listeners: [ tcp-main ]
      service: postgres
      tls_mode: passthrough # Default `None`, can be left blank or one of `terminate`/`passthrough`
```

### 3. Run PortIQ

```bash
./target/release/portiq --config portiq.yml
```

## Usage

Once PortIQ is running, you can send requests to it, and it will route them to the appropriate upstream service based on
the configuration.

**Example:**

```bash
# Assuming `https-main` listener is running on https://localhost:3443
curl https://localhost:3443/api/v1/users
```

This request will be distributed between `https://user.service1:4443` and `https://user.service1:5443` based on weights.

## Configuration Options

| Section         | Key           | Description                                     |
|-----------------|---------------|-------------------------------------------------|
| **version**     | `version`     | Configuration version (currently 1)             |
| **admin_api**   | `addr`        | Address and port, default is `127.0.0.1:5678`   |
| **log**         | `level`       | `DEBUG`, `INFO`, `WARN`, `ERROR`                |
|                 | `format`      | `compact` or `json`                             |
|                 | `file_path`   | `stdout` or a file path                         |
| **access_log**  | `enabled`     | `true` or `false`                               |
|                 | `format`      | `compact` or `json`                             |
|                 | `file_path`   | `stdout` or a file path                         |
| **listeners**   | `name`        | Name of the listener                            |
|                 | `addr`        | Address and port to bind (e.g., `0.0.0.0:3000`) |
|                 | `protocol`    | `http` or `https`                               |
| **tls**         | `cert_file`   | Path to certificate .pem file                   |
|                 | `key_file`    | Path to private key .pem file                   |
|                 | `default`     | Whether this is the default certificate         |
|                 | `hostnames`   | List of hostnames for SNI routing               |
| **http**        | `http`        | Container for HTTP-related configuration        |
| **middlewares** | `middlewares` | HTTP middleware configurations                  |
| **services**    | `upstreams`   | List of backend servers                         |
|                 | `target`      | URL of the backend server                       |
|                 | `weight`      | Weight for the WRR load balancer                |
| **routes**      | `hosts`       | List of hostnames to match                      |
|                 | `path`        | URL path to match                               |
|                 | `listeners`   | List of listeners this route applies to         |
|                 | `service`     | Name of the service to route to                 |
|                 | `middlewares` | List of middleware to apply                     |

## Roadmap

This project is still in its early stages, but the following features are planned for future development:

- **Robust Error Handling:** Implement comprehensive error handling for network issues, upstream failures, and invalid
  client requests. Add validation for configuration.
- **Comprehensive Unit Tests:** Develop a full suite of unit tests for individual components like routing, load
  balancing and middlewares.
- **Upstream Health Checks:** To ensure traffic is only sent to healthy backend services.
- **Expose Metrics:** To integrate with monitoring solutions like Prometheus.

Feedback and contributions to these or any other features are highly welcome.

## Contributing

This project is a work in progress and was built primarily for learning purposes. I welcome any and all feedback,
suggestions, and contributions! If you have any ideas for improvement, please feel free to open an issue or submit a
pull request.

## License

This project is licensed under the [MIT License](LICENSE).
