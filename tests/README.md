# Test Fixtures

This directory contains binary fixtures used by integration tests.

## Dependences

`sudo apt install p7zip-full`

## Regenerating Fixtures

```bash
cargo test --test fixtures -- --ignored
```

## Files

- `empty.bale` - Empty bale archive (22-byte EOCD only)
