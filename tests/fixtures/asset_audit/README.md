# Asset audit contract fixtures

`complete/` is one reusable project for the audit tests. Its world document references `assets/referenced.png`. The project manifest declares `assets/dynamic.png` as a dynamic path, ignores `assets/ignored/**`, and sets every optional budget.

| File | Intended case |
| --- | --- |
| `assets/referenced.png` | Known world reference and valid 1 × 1 PNG |
| `assets/dynamic.png` | Explicit dynamic path and valid 1 × 1 PNG |
| `assets/ignored/skip.png` | Excluded from orphan and budget checks |
| `assets/orphan.wav` | Unreferenced valid PCM WAV |
| `assets/malformed.wav` | RIFF/WAVE prefix with a missing `fmt ` and data header; expected `invalid_media_header` |
| `assets/mismatch.mp3` | Valid PCM WAV bytes with an MP3 extension; expected `extension_mismatch` |
| `assets/malformed.png` | PNG signature with no complete header |
| `assets/mismatch.jpg` | Valid PNG bytes with a JPEG extension |
| `assets/large.bin` | 192 bytes, above the 128 byte file limit |
| `assets/wide.png` | Valid 4 × 2 PNG, above the dimension limit of 2 |

The current inventory contains ten asset files totaling 650 bytes; it uses the world document to find references without packing that document. The total asset bytes exceed the 400 byte aggregate limit, and the valid PNG pixel total exceeds the limit of 8. Task 2 tests should use these files to assert the audit findings and the budget scope. `missing-dynamic/` declares an absent path for `declared_dynamic_path_missing`. `invalid-traversal/` and `invalid-glob/` cover rejected manifest paths. Symlinks are created in temporary directories by tests because their targets depend on the test host. Task 3 tests should use `complete/` to assert the command's single JSON report on stdout.
