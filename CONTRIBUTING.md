# Contributing to bale

Thank you for your interest in contributing to `bale`! This document provides guidelines for contributing to the project.

## Code of Conduct

This project adheres to the Contributor Covenant [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## How to Contribute

### Reporting Bugs

If you find a bug, please create an issue with:

- A clear, descriptive title
- Steps to reproduce the problem
- Expected vs actual behavior
- Your environment (OS, Rust version, `bale` version)
- Any relevant error messages or logs

### Suggesting Enhancements

Enhancement suggestions are welcome! Please create an issue describing:

- The motivation for the enhancement
- A clear description of the proposed functionality
- Any potential implementation considerations

### Pull Requests

1. **Fork and Clone**: Fork the repository and clone it locally
2. **Create a Branch**: Create a feature branch from `main`
3. **Make Changes**: Follow the project's coding standards (see below)
4. **Test**: Ensure all tests pass with `cargo test`
5. **Lint**: Run `cargo clippy` and address any warnings
6. **Commit**: Write clear, descriptive commit messages
7. **Push**: Push your branch to your fork
8. **Open a PR**: Submit a pull request to the `main` branch

## Development Setup

```bash
# Clone your fork
git clone https://github.com/YOUR-USERNAME/bale.git
cd bale

# Build the project
cargo build

# Run tests
cargo nextest run  # preferred
cargo test         # alternative

# Run clippy
cargo clippy

# Run all pre-commit checks
git precommit --all
```

## Coding Standards

This project maintains strict code quality standards:

### Required Practices

- **Comprehensive documentation**: All items (public and private) must be documented
  - Include `# Errors`, `# Panics`, and `# Safety` sections where applicable
- **No direct output**: Do not use `println!` or `eprintln!` in library code
- **One item per file**: Generally, place one public item (struct, enum, or trait) per file
  - File names should match the item name
  - Include a comment if violating this guideline

### Code Organization

- **Module structure**: `mod.rs` files should only contain module declarations and re-exports
- **Error handling**: Use `Result` types with `BaleError`
- **Testing**: Add tests for new functionality in `#[cfg(test)]` modules
- **Feature flags**: Use feature flags for optional functionality (fuse, compact)

### Whitelist .gitignore

This project uses a whitelist approach to version control. Only explicitly allowed files are tracked. When adding new file types, update `.gitignore` to include them with specific extensions.

## Testing

- Write unit tests for new functionality
- Test different feature flag combinations
- Ensure tests pass with `cargo test`
- Consider property-based tests with proptest for edge cases
- FUSE tests require fusermount3 to be available

## Documentation

- Update README.md for user-facing changes
- Add rustdoc comments for all public APIs
- Include examples in documentation where helpful
- Run `cargo doc --open` to preview documentation

## Dependencies

- Keep dependencies minimal and well-vetted
- Discuss new dependencies before adding them
- Pin exact versions (e.g., `"2.0.17"` not `"2"`)
- All dependencies belong in the workspace root `Cargo.toml`

## Questions?

If you have questions about contributing, feel free to:

- Open an issue for discussion
- Check existing issues and pull requests for context

Thank you for contributing to `bale`!
