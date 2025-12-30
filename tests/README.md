# Test Fixtures

This directory contains binary fixtures used by integration tests.

## Regenerating Fixtures

```bash
cargo test --test fixtures -- --ignored
```

## Files

- `empty.bale` - Empty bale archive (22-byte EOCD only)
- `single_file.bale` - Archive with single "hello.txt" containing "Hello, World!"
