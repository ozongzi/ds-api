# Contributing to ds-api

Thank you for your interest in contributing to ds-api! This document provides guidelines and instructions for contributing to this project.

## Code of Conduct

Please be respectful and considerate of others when contributing to this project. We aim to foster an inclusive and welcoming community.

## How to Contribute

### Reporting Bugs

If you find a bug, please create an issue with the following information:

1. **Description**: Clear and concise description of the bug
2. **Steps to Reproduce**: Step-by-step instructions to reproduce the issue
3. **Expected Behavior**: What you expected to happen
4. **Actual Behavior**: What actually happened
5. **Environment**: 
   - Rust version (`rustc --version`)
   - Operating system
   - ds-api version
6. **Additional Context**: Any other relevant information

### Suggesting Features

Feature suggestions are welcome! Please create an issue with:

1. **Feature Description**: Clear description of the proposed feature
2. **Use Case**: How this feature would be used
3. **Alternatives Considered**: Any alternative solutions you've considered
4. **Additional Context**: Any other relevant information

### Pull Requests

We welcome pull requests! Here's how to submit one:

1. **Fork the Repository**: Create your own fork of the project
2. **Create a Branch**: Create a feature branch for your changes
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Make Your Changes**: Implement your feature or bug fix
4. **Add Tests**: Ensure your changes are covered by tests
5. **Update Documentation**: Update relevant documentation
6. **Run Tests**: Make sure all tests pass
   ```bash
   cargo test
   ```
7. **Check Formatting**: Ensure code is properly formatted
   ```bash
   cargo fmt --check
   ```
8. **Check Lints**: Run clippy to catch common issues
   ```bash
   cargo clippy -- -D warnings
   ```
9. **Commit Changes**: Write clear, descriptive commit messages
10. **Push to Your Fork**: Push your changes to your fork
11. **Create Pull Request**: Open a pull request against the main repository

## Development Setup

### Prerequisites

- Rust 1.70 or later
- Cargo (comes with Rust)

### Getting Started

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/ds-api.git
   cd ds-api
   ```

2. Install dependencies:
   ```bash
   cargo build
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Build documentation:
   ```bash
   cargo doc --open
   ```

### Running Examples

Examples require a DeepSeek API token. Set it as an environment variable:

```bash
export DEEPSEEK_API_KEY="your-api-key-here"
```

Then run an example:

```bash
cargo run --example basic_usage
```

## Coding Standards

### Code Style

- Follow Rust formatting standards using `cargo fmt`
- Use meaningful variable and function names
- Write clear comments for complex logic
- Keep functions focused and single-purpose

### Documentation

- All public items must have documentation comments
- Use examples in documentation where appropriate
- Keep documentation up-to-date with code changes
- Write documentation in English

### Testing

- Write unit tests for new functionality
- Ensure tests are independent and repeatable
- Test edge cases and error conditions
- Maintain or improve test coverage

### Error Handling

- Use appropriate error types
- Provide helpful error messages
- Use `Result` for fallible operations
- Document error conditions

## Project Structure

```
ds-api/
├── src/
│   ├── lib.rs              # Main library entry point
│   ├── request.rs          # Request builder
│   ├── response.rs         # Response trait
│   ├── normal_chatter.rs   # Custom history chatter
│   ├── simple_chatter.rs   # Simple chatter
│   └── raw/               # Raw API structures
│       ├── mod.rs
│       ├── request/
│       └── response/
├── examples/              # Usage examples
├── tests/                # Integration tests
├── Cargo.toml           # Project configuration
├── README.md            # Project documentation
├── CHANGELOG.md         # Version history
└── CONTRIBUTING.md      # This file
```

## Release Process

1. **Version Bump**: Update version in `Cargo.toml`
2. **Update CHANGELOG**: Add release notes
3. **Run Tests**: Ensure all tests pass
4. **Check Documentation**: Build and verify documentation
5. **Create Tag**: Create git tag for the release
6. **Publish to crates.io**: `cargo publish`

## Getting Help

If you need help with contributing:

1. Check the existing documentation
2. Look at existing issues and pull requests
3. Ask questions in issues
4. Review the examples for usage patterns

## License

By contributing to ds-api, you agree that your contributions will be licensed under the project's MIT OR Apache-2.0 dual license.

## Acknowledgments

Thank you for contributing to ds-api! Your efforts help make this project better for everyone.
```

现在让我检查一下整个项目的完整性。让我运行一个完整的构建和测试：