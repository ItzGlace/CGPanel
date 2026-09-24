# Contributing

Use a dedicated Ubuntu 24.04 development VM for host operations. Never run live provisioning tests against a production server.

Keep host operations in the Rust broker. Validate at both the HTTP boundary and privileged boundary. Avoid passing tenant input to a host shell. Changes to account ownership, SQL grants, DNS rendering, firewall rules, or filesystem boundaries require meaningful negative tests.

Run formatting, Clippy with warnings denied, unit tests, and shell syntax checks before submitting a change. Describe any live integration tests and the host environment used. Never include secrets or live customer information in fixtures.

Contributions are licensed under AGPL-3.0-or-later. Prefer focused changes with a clear explanation of the resulting behavior and relevant validation.
