# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

No unreleased changes.

## [0.2.0] - 2026-02-27

### Added
- Refactored and safer high-level API:
  - `Response::content()` now returns `Result<&str, ApiError>` (breaking change).
  - `SimpleChatter::system_prompt_mut()` now returns `Option<&mut String>` to avoid panics.
  - Reworked `Request::execute_*` family for a more consistent API: unified base URL handling, methods accept `&reqwest::Client` for reuse, and added `execute_client_streaming_baseurl` for testing/custom base URLs.
- Streaming improvements:
  - SSE (EventSource) handling improved; stream-layer errors are now returned as `ApiError::EventSource(String)`.
  - JSON parse errors in streaming chunks map to `ApiError::Json`.
- Tests & CI:
  - Added wiremock-based integration tests for non-streaming and streaming scenarios.
  - Added GitHub Actions CI workflow to enforce `cargo fmt --check`, `cargo clippy` and run the test suite.
- Documentation:
  - README updated with an example showing how to handle `ApiError::EventSource` and streaming error strategies.
  - Module docs and examples updated to match the new APIs.

### Fixed
- Eliminated panics caused by `unwrap()` and direct indexing; replaced with safe checks and explicit `ApiError` results.
- Removed incorrect `/v1` usage (DeepSeek API does not use a `/v1` prefix); default base URL is `https://api.deepseek.com`.

### Breaking changes
- `Response::content()` changed from returning `&str` to `Result<&str, ApiError>`. Call sites must handle the `Result`.
- `SimpleChatter::system_prompt_mut()` changed from `&mut String` to `Option<&mut String>`.
- Signatures and behavior of `Request::execute_*` methods changed (now accept `&Client`, unified base URL handling). Update call sites accordingly.


## [0.1.0] - 2024-01-01

### Initial Release
- First public release of ds-api
- Basic chat completion functionality
- Support for DeepSeek API v1
- Core request/response structures
- Basic error handling
- Minimal documentation

### Technical Details
- Built with Rust 2024 edition
- Dependencies: reqwest, tokio, serde, futures, eventsource-stream
- MIT OR Apache-2.0 dual license
- Comprehensive test suite
- Examples for common use cases

## Upgrade Guide

### From Pre-release versions
This is the first public release. No upgrade guide needed.

## Deprecations
None in this release.

## Security
- API tokens should be stored securely using environment variables
- No sensitive data is logged by default
- All HTTP requests use HTTPS
- Error messages avoid exposing sensitive information

## Acknowledgments
- Thanks to the DeepSeek team for their excellent API
- Inspired by OpenAI's Rust client libraries
- Built with the Rust community's excellent crates

## Contributing
Please see CONTRIBUTING.md for guidelines on how to contribute to this project.

## License
This project is licensed under either MIT or Apache-2.0 at your option.