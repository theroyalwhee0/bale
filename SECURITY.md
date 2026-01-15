# Security Policy

## Supported Versions

The following versions of `bale` are currently supported with security updates:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

## Reporting a Vulnerability

We take the security of `bale` seriously.

### How to Report

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report security vulnerabilities via one of the following methods:

1. **GitHub Security Advisories** (preferred): Use GitHub's [private vulnerability reporting](https://github.com/theroyalwhee0/bale/security/advisories/new)
2. **Email**: Send to [security@theroyalwhee.com](mailto:security@theroyalwhee.com). See <https://www.theroyalwhee.com/security/policy/>

### What to Include

Please include as much of the following information as possible:

- Type of vulnerability (e.g., path traversal, archive parsing, etc.)
- Step-by-step instructions to reproduce the issue
- Affected versions or commits
- Potential impact of the vulnerability
- Any suggested fixes or mitigations

### What to Expect

- **Initial Response**: Within 72 hours of your report
- **Status Update**: Within 2 weeks
- **Fix Timeline**: Based on severity and complexity
- **Credit**: Security researchers will be credited in release notes unless they prefer to remain anonymous

### Security Scope

Security issues of particular concern for `bale` include:

- Path traversal vulnerabilities in archive extraction
- Zip bomb or decompression bomb attacks
- Memory safety issues in mmap handling
- Arbitrary file overwrite during extraction
- Symlink attacks
- FUSE filesystem escape vulnerabilities
- Filename validation bypass allowing dangerous paths
- Dependency vulnerabilities
- Supply chain security issues

Thank you for helping keep `bale` and its users safe!
