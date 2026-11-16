# Continuous integration checklist

GitHub Actions workflow installation requires authorization with the
GitHub workflow permission. The clean-history first commit intentionally
contains no enabled workflows.

Run protocol-sync verification, TypeScript build/tests, Core HTTP
interoperability, Python/Go/Rust unit tests and a clean-package audit.
Cloud-only client verification requires mocked API capabilities or a
consented Cloud test environment; never require proprietary server
implementations in the public repository.
