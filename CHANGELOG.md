# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial release of ds-api library
- Support for DeepSeek Chat and DeepSeek Reasoner models
- Streaming and non-streaming API responses
- Tool calling support with function definitions
- JSON mode for structured responses
- Builder pattern for request construction
- SimpleChatter and NormalChatter for easy conversation management
- Comprehensive documentation with examples
- Raw API structures for direct API interaction

### Features
- Type-safe request building with validation
- Asynchronous API calls using tokio and reqwest
- Server-Sent Events (SSE) streaming support
- Custom history management via History trait
- Error handling with Box<dyn Error>
- Complete serde serialization/deserialization support
- Extensive test coverage
- Detailed API documentation

### Documentation
- Complete Rustdoc documentation for all modules
- README with usage examples and installation instructions
- Comprehensive examples demonstrating all features
- Module-level documentation explaining architecture
- Inline code examples in documentation

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