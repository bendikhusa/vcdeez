# Reconstructed WLF format: v172

This is a reverse-engineering note, not an official Siemens specification.
Evidence comes from `vsim.wlf`, local converter/SDK disassembly, named functions
and DWARF metadata in the local static SDK archive, and comparison with an SDK
VCD export. The standalone Rust converter does not execute or link that SDK.

The sample is 49,152 bytes. Its native export matches all 189 named signal traces
in the SDK reference: 7,149 VCD value changes and eight record scopes. Only this
sample has full signal-trace validation. Broader opcode families recovered from
debug metadata do not imply full support for their payloads.

Wellen 0.25.5 successfully loads all 189 exported signals. The native output
contains 29 scopes, including an empty generate container omitted by the SDK
reference, which contains 28. Signal names and value traces still match.

## File structure and primitive encoding

Multi-byte integers are big-endian. Signed fields use two's complement. Files
consist of 8,192-byte parcels, beginning with a header parcel. Decoded payloads
contain context bytecode (types and hierarchy) or archive bytecode (time/value
events). These bytecode namespaces are distinct.

Strings have a one-byte length followed by bytes, with no trailing NUL. The
reader implements a `0xff` escape followed by a `u32` length; this long-string
case has not been exercised by the sample. Names currently must be UTF-8.

### Header

| Offset | Width | Meaning / sample value |
|---|---:|---|
| `0x00` | 2 | Signature `0x1076`; reader also recognizes `0x1364` |
| `0x02` | 2 | Version 172 (`0x00ac`) |
| `0x04` | 2 | Secondary version field 100; exact role unresolved |
| `0x06` | 4 | Resolution code 3, corresponding to `1ps` |
| `0x0a` | 8 | Recorded end time 1,406,250 |
| `0x12` | 4 | Recorded end delta 2 |
| `0x16` | 4 | Type count 28 |
| `0x1a` | 4 | Unresolved field, 0 |
| `0x1e` | 4 | Maximum/count of archive slots, 580 |
| `0x22` | 4 | Unresolved field, 7 |
| `0x26` | 4 | Unresolved field, 0 |
| `0x2a` | 4 | Unresolved field, 12 |

The rest of the header includes metadata and strings and is not fully described.
The implemented time-resolution mapping starts at `0 = 1fs` and advances by
powers of ten through `15 = 1s`. Sample code `3 = 1ps` matches the reference.

### Single-parcel buffers

| Offset within parcel | Width | Meaning / observation |
|---|---:|---|
| 0 | 2 | Tag `0xffff` |
| 2 | 2 | Parcel size field; not used by current extraction |
| 4 | 4 | Signed encoded payload length |
| 8 | 4 | Bookkeeping field, observed 2 |
| 12 | 4 | Bookkeeping field; exact role unresolved |
| 16 | variable | Payload |

Positive length means compressed bytes; negative length means uncompressed
bytes with the absolute length. The payload must fit the parcel. Compressed
streams have an explicit terminator and may leave a trailing byte unused.
Multi-parcel tags `0xfffd`, `0xfffc`, and `0xfff9` were observed in earlier
probes, but their assembly/trailer rules are not implemented.

The sample's decoded buffers are:

| Parcel | File offset | Decoded bytes | Observed role |
|---:|---:|---:|---|
| 0 | `0x0000` | — | Header |
| 1 | `0x2000` | 390 | Channel directory, starts with `0x1b` directives |
| 2 | `0x4000` | 11,123 | Types and hierarchy |
| 3 | `0x6000` | 38 | Archive/time index |
| 4 | `0x8000` | — | Reserved/mostly zero |
| 5 | `0xa000` | 9,135 | Event stream |

Channel and index structures remain incomplete. Current export identifies one
context buffer starting with `0x3d` or `0x08`, and one event buffer starting with
`0x1c`. Multiple/ambiguous streams are rejected. A complete reader should follow
the directory instead of identifying buffers this way.

## Compression v1

Control bits are interleaved with payload bytes and read least-significant-bit
first. Start with `control = 2`. To read a bit, shift control right once; if it
becomes 1, replace it with the next input byte plus 256. Return `control & 1`.

A zero token bit emits the next input byte literally. A one token bit reads
two bytes and initializes:

```text
offset = ((first & 0xf8) << 5) | second
length = first & 7
```

Extend offset, then length. For each field, while the next control bit is 1,
read another bit and set `field = field * 2 + bit`, wrapping at 32 bits. A zero
control bit ends that extension sequence.

If the final offset has bit 31 set, stop. Otherwise copy `length + 3` bytes from
distance `offset + 1` behind the output cursor. Copies may overlap. The length
addition wraps at 32 bits; a wrapped count of zero is a no-op in the analyzed
implementation. vcdeez validates distances and caps decompression at 64 MiB.

The native encoder emits literal-only tokens and a terminator. It exercises the
compression codec but is not a complete WLF file writer.

## Context bytecode

These layouts describe the sample's version path. `string` means the encoding
above. Fields named metadata do not yet have confirmed individual semantics.

### File properties

`0x3d` introduces a one-byte subcode: 0 carries a string describing the argument
region, 1 carries a `u32` argument count, 2 carries an argument string, and 3 ends
the description without a payload. `0x23` declares a source file using
`u32 file_id`, `string path`, and `u32 metadata`.

### Regions

`0x08` begins a region:

```text
string name
u32    object metadata
u8     flags
u8     region_kind
...    kind-specific payload
```

| Kind | Observed meaning | Additional payload |
|---:|---|---|
| 11 | Root `/` | None |
| 1 | VHDL design region | Three strings: library location, entity, architecture; then source-file information |
| 31 | For-generate container | `u32` type reference, then two `u32` source-location fields |
| 4 | Generate instance | Two `u32` source-location fields |

Kind 1 source-file information starts with `u32 count`. A nonzero count is
followed by that many `u32` file references and six further `u32` metadata
fields. A zero count has no trailing payload in the analyzed function.

`0x09` ends a region. `0x1c` terminates the complete context buffer.

### Type definitions

| Opcode | Type | Payload after opcode |
|---:|---|---|
| `0x03` | Integer/scalar | `u32 id`, `i32 left`, `i32 right`, `u32 properties`, `u32 value_size` |
| `0x05` | Enumeration | `u32 id`, `u32 left`, `u32 right`, `u8 direction`, `u32 properties`, literal strings |
| `0x06` | Array | `u32 id`, `i32 left`, `i32 right`, `u8 direction`, `u32 element_type`, `u32 index_type`, `u8 flags`, `u32 properties` |
| `0x07` | Record | `u32 id`, `u8 flags`, `u32 properties`, `u32 field_count`, fields |

Type references are `u32` IDs. Enum definitions store `max(left, right) + 1`
strings, indexed by ordinal. The sample includes a nine-state logic enum
`['U', 'X', '0', '1', 'Z', 'W', 'L', 'H', '-']` and an 11-literal FSM enum
with names such as `s_idle`. The current scalar reader accepts properties 0
and size 4, and renders its integer values with sign extension to 64 VCD bits.

Record fields are pairs of `string field_name` and `u32 field_type`. For example,
type 2 contains:

```text
data -> type 3, array [32:0] of type 1
addr -> type 4, array [16:0] of type 1
ack  -> type 1
stb  -> type 1
we   -> type 1
```

Scalar types occupy one archive slot. Supported arrays concatenate slots in
element order, with `abs(left - right) + 1` elements. Records concatenate slots
in field order. Thus this record occupies 53 slots: 33 + 17 + 1 + 1 + 1.
This field/type construction is what an ordinary flattened VCD loses.

Null arrays, additional scalar encodings, access/file types, dynamic arrays,
classes, unions, and many further type directives are not implemented. Type
property semantics remain incomplete and are retained through the capsule.

### Signals and aliases

`0x0a` declares a signal:

```text
string name
u32    object metadata
u32    symbol-kind metadata
u32    type_id
u8     mode
u8     attributes
u32    source_file
u32    source_line
i32    archive_count
...    archive mapping
u32    selector_count
...    selectors
```

Positive archive count means one `u32 first_archive` and that many consecutive
slots. Negative count means `abs(count)` explicit `u32` archive IDs. Zero has
no archive mapping; current export rejects nonempty types without slots.
`wb_in` occupies 53 slots starting at archive 10; `wb_out` begins at 63.

`0x10` declares an alias. Leading fields are the same, but a `u32` archive
reference replaces the signal's count/mapping. Selector count follows. With no
selectors, the native reader derives the slot count from the type and uses the
reference as the first archive. This is verified for the sample's scalar and
vector aliases. Nonzero selectors are rejected.

Archives are one-based. Several declarations can reference the same slots,
so each archive update must notify every subscribed VCD signal.

## Event bytecode

The event namespace differs from context: event opcode `0x1c` sets absolute time,
whereas context opcode `0x1c` ends the context. The sample contains 2,035 archive
value records, expanding through aliases/composites to 7,149 VCD value changes.

### Time and control

| Opcode | Meaning | Payload |
|---:|---|---|
| `0x1a` (26) | EOF | None |
| `0x1b` (27) | Relative time | `u8` increment |
| `0x1c` (28) | Absolute time | `u64` timestamp |
| `0x1d` (29) | Delta cycle | `u8` absolute delta |
| `0x1e` (30) | Delta cycle | `u32` absolute delta |
| `0x54` (84) | Reset archive cursor | None |
| `0x55` (85) | Temporary/buffer-transition time marker | No inline payload on sample's version path |
| `0x56` (86) | Relative time | `u16` increment |
| `0x57` (87) | Relative time | `u32` increment |

Time advances reset the delta to zero. Delta values are assigned, not added.
Relative archive offsets are signed, including one-byte offsets. The sample starts:

```text
1c 00 00 00 00 00 00 00 00   time 0
0d 01 00                     absolute archive 1, byte value 0
2c 01 00                     archive cursor +1, byte value 0
```

### Numeric event families

Names recovered from debug metadata identify four 12-opcode families:

| Decimal opcodes | Archive addressing | Family |
|---|---|---|
| 0–11 | Absolute | `EVENT` |
| 12–23 | Absolute | `VALUE` |
| 31–42 | Relative signed | `REL_EVENT` |
| 43–54 | Relative signed | `REL_VALUE` |

Subtract the family's first opcode to obtain `index`. Archive-number width is
selected by `index / 4`: 0 means 1 byte, 1 means 2 bytes, 2 means 4 bytes. Value
width is selected by `index % 4`: 0 means none, 1 means 1 byte, 2 means 4 bytes,
3 means 8 bytes. Both fields are big-endian. Absolute archive numbers replace
the cursor; relative offsets add to it.

`VALUE` supplies initialization values; `EVENT` supplies updates. The exporter
processes both as values and rejects valueless combinations. The sample exercises
opcodes 13, 44, 45, 32, and 36, together with absolute time, `u16` relative time,
byte deltas, the temporary marker, and EOF. Logic values are literal ordinals:
0 means `'U'`, 2 means `'0'`, 3 means `'1'`, and 4 means `'Z'`.

Recovered but unimplemented families include register word pairs (57–68),
messages (69), function values (70–81), checkpoint markers (82–83), start-log
value kind (88), compact zero/one/X/Z events (89–112), and clock start (113).
Their existence does not establish complete payload semantics.

## Record-preserving VCD export

A record becomes a scope, and each field becomes a variable or nested scope:

```vcd
$scope module wb_in $end
$comment vcdeez record wb_in $end
$var wire 33 * data [32:0] $end
$var wire 17 + addr [16:0] $end
$var wire 1 , ack $end
$var wire 1 - stb $end
$var wire 1 . we $end
$upscope $end
```

Arrays of records become an array scope with `(index)` children. Logic arrays
become vectors. Other enum literals become VCD string values. Root `/` is omitted.
VCD's four states map `'U'`, `'W'`, and `'-'` to `x`, `'L'` to `0`, and `'H'`
to `1`. Original nine-state values remain in the recovery capsule.

Each raw update is emitted in order, including updates sharing a time and delta.
Coalescing a delta discarded intermediate vector values during validation.
Delta changes also use `$comment vcdeez delta N $end`; ordinary viewers need
not interpret those comments.

## Exact roundtrip capsule

The exporter prepends valid VCD comments:

```text
$comment vcdeez.wlf.v1 header BYTE_COUNT WLF_SHA256 VCD_BODY_SHA256 $end
$comment vcdeez.wlf.v1 chunk 0 HEX_BYTES $end
$comment vcdeez.wlf.v1 chunk 1 HEX_BYTES $end
```

Each chunk contains up to 1,024 original WLF bytes encoded as lowercase hex.
Indices are consecutive from zero. Hashes cover the original WLF and the exact
VCD body, including comments and newlines. Restoration checks ordering, byte
count, both hashes, and the recovered WLF header, then writes the original bytes.
Edited VCD text is rejected.

This preserves original type IDs, source metadata, nine-state values, compression
choices, and information that is not yet semantically decoded. It is recovery
of an embedded original, not generation of a new WLF from arbitrary or edited
VCD. Editable semantic roundtrip still requires full channel/index writing,
event encoders, and metadata for rebuilding types without embedding the file.

## Implementation and open work

- [src/native.rs](src/native.rs): header, parcels, compression.
- [src/wlf.rs](src/wlf.rs): context, events, VCD hierarchy.
- [src/capsule.rs](src/capsule.rs): recognized capsule and integrity checks.
- `analysis/symbols.py` and `analysis/events.py`: research probes, not runtime
  dependencies or authoritative alternate decoders.

Open work includes multi-parcel assembly, full directory/index navigation,
additional versions, regions, types, selectors, specialized event formats, null
array handling, and writing new WLF from semantic VCD data. Unsupported directives
return errors rather than fabricated waveforms. Broader integer encoding still
needs independent negative-value and width probes.
