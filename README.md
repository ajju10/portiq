# Portiq: A High-Performance HTTP(S) API Gateway in Rust

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.79%2B-orange.svg)](https://www.rust-lang.org/)

**Portiq** is an HTTP(S) API gateway written in Rust, built using `hyper`, `rustls`, and `tokio`.

> **Note:** This project was built primarily for learning purposes to explore Rust for network programming.

## Features

- **Path-based Routing**: Route requests to different upstream services based on the URL path.
- **TLS Termination**: Offload TLS encryption/decryption from your backend services.
- **Load Balancing**: In-memory Weighted Round Robin (WRR) for distributing traffic.
- **Middleware**:
  - Request ID for tracking
  - Access logger for detailed request logs
- **Static Configuration**: Configure everything through a single `portiq.yml` file.
- **Logging**: Structured and configurable logging for better monitoring.

## Getting Started

### Prerequisites

- **Rust Toolchain**: The Minimum Supported Rust Version (MSRV) is `1.79` or later. This is determined by the MSRV of its core dependencies (`tokio`, `hyper`, `rustls`).
- **OpenSSL Development Libraries**
- **Build Tools**: `rustls` uses `aws-lc-rs` as the default cryptography provider, which requires a C compiler (like GCC or Clang) and `cmake` to build. Please see the `aws-lc-rs` documentation for more details.

### 1. Build from Source

```bash
# Clone the repository
git clone https://github.com/ajju10/portiq.git
cd portiq

# Build in release mode
cargo build --release
```

### 2. Configure Portiq

Create a `portiq.yml` file. Here's a sample configuration:

```yaml
server:
  host: 127.0.0.1
  port: 8000
  protocol: http

log:
  level: INFO
  format: common
  file_path: stdout

access_log:
  enabled: true
  format: common
  file_path: stdout

routes:
  - path: /api/users
    methods: [GET, POST]
    upstream:
      - url: http://localhost:5000
        weight: 2
      - url: http://localhost:5001
        weight: 1
```

### 3. Run Portiq

```bash
./target/release/portiq portiq.yml
```

## Usage

Once Portiq is running, you can send requests to it, and it will route them to the appropriate upstream service.

**Example:**

```bash
# Assuming Portiq is running on localhost:8000
curl http://localhost:8000/api/users
```

This request will be forwarded to either `http://localhost:5000` or `http://localhost:5001` based on the WRR load balancing.

## Configuration Options

| Section        | Key         | Description                                            |
|----------------|-------------|--------------------------------------------------------|
| **server**     | `host`      | Binding address for the server.                        |
|                | `port`      | Listening port.                                        |
|                | `protocol`  | `http` or `https`.                                     |
|                | `cert_file` | Path to the SSL certificate `.pem` file (for `https`). |
|                | `key_file`  | Path to the SSL private key `.pem` file (for `https`). |
| **log**        | `level`     | `DEBUG`, `INFO`, `WARN`, `ERROR`.                      |
|                | `format`    | `common` or `json`.                                    |
|                | `file_path` | `stdout`, `stderr`, or a file path.                    |
| **access_log** | `enabled`   | `true` or `false`.                                     |
|                | `format`    | `common` or `json`.                                    |
|                | `file_path` | `stdout`, `stderr`, or a file path.                    |
| **routes**     | `path`      | URL path to match.                                     |
|                | `methods`   | List of allowed HTTP methods (e.g., `[GET, POST]`).    |
|                | `upstream`  | List of backend servers.                               |
|                | `url`       | URL of the backend server.                             |
|                | `weight`    | Weight for the WRR load balancer.                      |

## Roadmap

This project is still in its early stages, but the following features are planned for future development:

- **Robust Error Handling:** Implement comprehensive error handling for network issues, upstream failures, and invalid client requests. Add validation for configuration.
- **Comprehensive Unit Tests:** Develop a full suite of unit tests for individual components like routing, load balancing and middlewares.
- **Upstream Health Checks:** To ensure traffic is only sent to healthy backend services.
- **Metrics Exposition:** To integrate with monitoring solutions like Prometheus.

Feedback and contributions to these or any other features are highly welcome.

## Contributing

This project is a work in progress and was built primarily for learning purposes. I welcome any and all feedback, suggestions, and contributions! If you have any ideas for improvement or have found a bug, please feel free to open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENSE).
