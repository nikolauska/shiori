# Test data

| File | Source |
| --- | --- |
| `anitomy.json` | `test/data.json` from https://github.com/erengy/anitomy, `develop` branch at `8498b538f5ddeedaef1b3fa5ad4bf6d67dd67d35`, MPL-2.0 |
| `anitomy-known-failures.txt` | Inputs from `anitomy.json` where the parser's output differs from the expected output. Regenerate with `UPDATE_KNOWN_FAILURES=1 cargo test -p shiori-core --test anitomy` and review the diff. |
