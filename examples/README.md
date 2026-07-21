# Examples

Canonical programs demonstrating mata-ll language features. For the broader
corpus of compiler try-outs and stress tests, see [`../experiments/`](../experiments/).

## Showcases

- `atdg.mll` — LZ4 decompression through the `contrib` library (`Lz4`, `Hex`);
  decodes an embedded blob and prints an ASCII comic.

  ```bash
  mll examples/atdg.mll -r
  ```
