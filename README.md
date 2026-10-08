# vcdeez

Standalone Rust WLF-to-VCD conversion that preserves VHDL records as VCD
scopes, keeping their fields grouped when imported into Surfer. No Questa
installation, Siemens library, or external converter is needed at runtime.

```sh
cargo build --release
./target/release/vcdeez wlf2vcd vsim.wlf standalone.vcd
surfer standalone.vcd
```

Expand Surfer's hierarchy to find records such as `wb_in`, `wb_rw`, and
`link_control`. Their fields appear beneath separate scopes. VCD expresses
the grouping through hierarchy; enumeration signals use VCD string values.

To restore the original WLF:

```sh
./target/release/vcdeez vcd2wlf standalone.vcd restored.wlf
cmp vsim.wlf restored.wlf
```

The exported VCD contains a recognized, SHA-256-checked reconstruction capsule
in `$comment` directives. Restoration is byte-for-byte exact because it contains
the original WLF bytes. This is **not a general VCD-to-WLF encoder**: ordinary
VCD input is rejected, and editing the exported VCD causes restoration to fail
instead of returning stale waveform data. Keep the original exported VCD for
roundtrip; a viewer's re-export may discard the comments.

Output files must not already exist. Use a fresh filename for each command.

## Current support

The native reader implements the subset of WLF v172 exercised by `vsim.wlf`:
single-parcel compressed/uncompressed buffers, VHDL hierarchy, 32-bit integers,
enum types, arrays, records, aliases, and numeric archive events. It produces
all 189 leaf signals and eight record scopes in that sample. All 7,149 exported
value changes were compared against the previous Siemens-SDK-based export;
every named signal trace matched, including intermediate changes at one time.
The generated VCD was also parsed with Wellen 0.25.5 and all 189 signals loaded.
The native hierarchy retains an additional empty generate-container scope,
giving 29 total scopes.

Other versions, multi-parcel buffers, additional region/type directives, and
specialized event encodings currently report an unsupported-format error.
This is a working standalone decoder for the supplied sample, not a complete
implementation of every WLF variant.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
./target/release/vcdeez inspect vsim.wlf
./target/release/vcdeez extract vsim.wlf decoded-blocks
```

See [WLF_FORMAT.md](WLF_FORMAT.md) for the reconstructed format and remaining
unknowns. The implementation has no Cargo dependencies.
