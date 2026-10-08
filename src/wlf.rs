//! Native WLF v172 context and archive bytecode reader.
//! Opcode layouts reconstructed from local binary analysis; no Siemens runtime.
use std::{collections::BTreeMap, fmt::Write};

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("WLF length overflow")?;
        let b = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| format!("truncated WLF at byte {}", self.pos))?;
        self.pos = end;
        Ok(b)
    }
    fn uint(&mut self, n: usize) -> Result<u64, String> {
        Ok(self
            .bytes(n)?
            .iter()
            .fold(0, |a, b| (a << 8) | u64::from(*b)))
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.uint(1)? as u8)
    }
    fn word(&mut self) -> Result<u32, String> {
        Ok(self.uint(4)? as u32)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.byte()?;
        let n = if n == 255 {
            self.word()? as usize
        } else {
            n as usize
        };
        String::from_utf8(self.bytes(n)?.to_vec()).map_err(|_| "WLF name is not UTF-8".into())
    }
}
#[derive(Clone, Debug)]
enum Type {
    Enum(Vec<String>),
    Integer,
    Array { left: i32, right: i32, element: u32 },
    Record(Vec<(String, u32)>),
}
#[derive(Debug)]
enum Node {
    Begin(String),
    End,
    Signal {
        name: String,
        ty: u32,
        archives: Vec<u32>,
    },
}
struct Context {
    types: BTreeMap<u32, Type>,
    nodes: Vec<Node>,
}
fn type_slots(id: u32, types: &BTreeMap<u32, Type>, depth: usize) -> Result<usize, String> {
    if depth > 128 {
        return Err("WLF type nesting exceeds 128".into());
    }
    match types
        .get(&id)
        .ok_or_else(|| format!("undefined WLF type {id}"))?
    {
        Type::Record(fields) => fields.iter().try_fold(0usize, |n, (_, t)| {
            n.checked_add(type_slots(*t, types, depth + 1)?)
                .ok_or("WLF type too large".into())
        }),
        Type::Array {
            left,
            right,
            element,
        } => {
            let n = (i64::from(*left) - i64::from(*right)).unsigned_abs() + 1;
            let count = type_slots(*element, types, depth + 1)?
                .checked_mul(n as usize)
                .ok_or("WLF array too large")?;
            if count > 1_000_000 {
                return Err("WLF array exceeds one million archive slots".into());
            }
            Ok(count)
        }
        _ => Ok(1),
    }
}
fn context(data: &[u8]) -> Result<Context, String> {
    let mut r = Reader { data, pos: 0 };
    let mut types = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut nesting = 0usize;
    while r.pos < data.len() {
        let start = r.pos;
        let op = r.byte()?;
        match op {
            0x3d => match r.byte()? {
                0 | 2 => {
                    r.string()?;
                }
                1 => {
                    r.word()?;
                }
                3 => {}
                sub => return Err(format!("unsupported WLF file-property {sub} at {start}")),
            },
            0x23 => {
                r.word()?;
                r.string()?;
                r.word()?;
            }
            8 => {
                let name = r.string()?;
                r.word()?;
                r.byte()?;
                let kind = r.byte()?;
                match kind {
                    1 => {
                        r.string()?;
                        r.string()?;
                        r.string()?;
                        let count = r.word()? as usize;
                        if count > 1_000_000 {
                            return Err("WLF source file count too large".into());
                        }
                        if count != 0 {
                            r.bytes(count * 4 + 24)?;
                        }
                    }
                    31 => {
                        r.word()?;
                        r.bytes(8)?;
                    }
                    4 => {
                        r.bytes(8)?;
                    }
                    11 => {}
                    _ => return Err(format!("unsupported WLF region kind {kind} at {start}")),
                }
                nesting += 1;
                if nesting > 128 {
                    return Err("WLF hierarchy exceeds 128 levels".into());
                }
                nodes.push(Node::Begin(name));
            }
            9 => {
                nesting = nesting.checked_sub(1).ok_or("unbalanced WLF region end")?;
                nodes.push(Node::End);
            }
            3 => {
                let id = r.word()?;
                r.word()?; // left bound
                r.word()?; // right bound
                let properties = r.word()?;
                let size = r.word()?;
                if properties != 0 || size != 4 {
                    return Err(format!(
                        "unsupported WLF scalar properties {properties:#x} / size {size}"
                    ));
                }
                if types.insert(id, Type::Integer).is_some() {
                    return Err("duplicate WLF type".into());
                }
            }
            5 => {
                let id = r.word()?;
                let left = r.word()?;
                let right = r.word()?;
                r.byte()?;
                r.word()?;
                let count = left.max(right) as usize + 1;
                if count > 1_000_000 {
                    return Err("WLF enum too large".into());
                }
                let literals = (0..count).map(|_| r.string()).collect::<Result<_, _>>()?;
                if types.insert(id, Type::Enum(literals)).is_some() {
                    return Err("duplicate WLF type".into());
                }
            }
            6 => {
                let id = r.word()?;
                let left = r.word()? as i32;
                let right = r.word()? as i32;
                r.byte()?;
                let element = r.word()?;
                r.word()?;
                r.byte()?;
                r.word()?;
                if types
                    .insert(
                        id,
                        Type::Array {
                            left,
                            right,
                            element,
                        },
                    )
                    .is_some()
                {
                    return Err("duplicate WLF type".into());
                }
            }
            7 => {
                let id = r.word()?;
                r.byte()?;
                r.word()?;
                let count = r.word()?;
                if count > 1_000_000 {
                    return Err("WLF record too large".into());
                }
                let fields = (0..count)
                    .map(|_| Ok((r.string()?, r.word()?)))
                    .collect::<Result<_, String>>()?;
                if types.insert(id, Type::Record(fields)).is_some() {
                    return Err("duplicate WLF type".into());
                }
            }
            10 | 16 => {
                let name = r.string()?;
                r.word()?;
                r.word()?;
                let ty = r.word()?;
                r.byte()?;
                r.byte()?;
                r.bytes(8)?;
                let n = r.word()? as i32;
                let slots = type_slots(ty, &types, 0)?;
                if slots > 1_000_000 || (op == 10 && n.unsigned_abs() > 1_000_000) {
                    return Err("WLF signal archive list too large".into());
                }
                let archives: Vec<u32> = if op == 16 {
                    let first = n as u32;
                    let end = first
                        .checked_add(slots as u32)
                        .ok_or("WLF alias archive overflow")?;
                    (first..end).collect()
                } else if n > 0 {
                    let first = r.word()?;
                    let end = first.checked_add(n as u32).ok_or("WLF archive overflow")?;
                    (first..end).collect()
                } else {
                    let count = n.unsigned_abs();
                    if count > 1_000_000 {
                        return Err("WLF archive list too large".into());
                    }
                    (0..count).map(|_| r.word()).collect::<Result<_, _>>()?
                };
                let selectors = r.word()?;
                if selectors != 0 {
                    return Err(format!("WLF signal selectors not yet supported at {start}"));
                }
                if slots != archives.len() {
                    return Err(format!(
                        "{name}: type needs {slots} archive slots, declaration has {}",
                        archives.len()
                    ));
                }
                nodes.push(Node::Signal { name, ty, archives });
            }
            0x1c => {
                if nesting != 0 {
                    return Err("unclosed WLF regions".into());
                }
                if r.pos != data.len() {
                    return Err("unexpected data after WLF context".into());
                }
                return Ok(Context { types, nodes });
            }
            _ => {
                return Err(format!(
                    "unsupported WLF context opcode {op:02x} at byte {start}"
                ))
            }
        }
    }
    Err("missing WLF context terminator".into())
}
fn logic(literals: &[String]) -> bool {
    !literals.is_empty()
        && literals.iter().all(|s| {
            let s = s.trim_matches('\'');
            s.len() == 1 && "UX01ZWLH-uxzwhl".contains(s)
        })
}
fn token(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_whitespace() || c == '$' {
                '_'
            } else {
                c
            }
        })
        .collect()
}
fn identifier(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.push(char::from((n % 94) as u8 + 33));
        n /= 94;
        if n == 0 {
            break;
        }
        n -= 1;
    }
    s
}
#[derive(Clone)]
enum Format {
    Bits(Vec<String>),
    Integer,
    Enum(Vec<String>),
}
struct Leaf {
    id: String,
    archives: Vec<u32>,
    format: Format,
}
struct Builder<'a> {
    types: &'a BTreeMap<u32, Type>,
    header: String,
    leaves: Vec<Leaf>,
    records: usize,
}
impl Builder<'_> {
    fn signal(
        &mut self,
        name: &str,
        ty: u32,
        archives: &[u32],
        depth: usize,
    ) -> Result<(), String> {
        if depth > 128 {
            return Err("WLF composite nesting exceeds 128".into());
        }
        let t = self.types.get(&ty).ok_or("missing WLF type")?.clone();
        let format = match t {
            Type::Record(fields) => {
                writeln!(
                    self.header,
                    "$scope module {} $end\n$comment vcdeez record {} $end",
                    token(name),
                    token(name)
                )
                .unwrap();
                self.records += 1;
                let mut pos = 0;
                for (field, id) in fields {
                    let count = type_slots(id, self.types, 0)?;
                    self.signal(&field, id, &archives[pos..pos + count], depth + 1)?;
                    pos += count;
                }
                self.header.push_str("$upscope $end\n");
                return Ok(());
            }
            Type::Array {
                left,
                right,
                element,
            } => {
                if let Some(Type::Enum(literals)) = self
                    .types
                    .get(&element)
                    .filter(|t| matches!(t,Type::Enum(l) if logic(l)))
                {
                    Format::Bits(literals.clone())
                } else {
                    writeln!(
                        self.header,
                        "$scope module {} $end\n$comment vcdeez array {} $end",
                        token(name),
                        token(name)
                    )
                    .unwrap();
                    let step = if left <= right { 1 } else { -1 };
                    let count = type_slots(element, self.types, 0)?;
                    let mut index = left;
                    for values in archives.chunks_exact(count) {
                        self.signal(&format!("({index})"), element, values, depth + 1)?;
                        index += step;
                    }
                    self.header.push_str("$upscope $end\n");
                    return Ok(());
                }
            }
            Type::Enum(literals) => {
                if logic(&literals) {
                    Format::Bits(literals)
                } else {
                    Format::Enum(literals)
                }
            }
            Type::Integer => Format::Integer,
        };
        let id = identifier(self.leaves.len());
        let (vcdtype, width) = match &format {
            Format::Bits(_) => ("wire", archives.len()),
            Format::Integer => ("integer", 64),
            Format::Enum(_) => ("string", 1),
        };
        let suffix = if let Some(Type::Array { left, right, .. }) = self.types.get(&ty) {
            format!(" [{left}:{right}]")
        } else {
            String::new()
        };
        writeln!(
            self.header,
            "$var {vcdtype} {width} {id} {}{suffix} $end",
            token(name)
        )
        .unwrap();
        if let Format::Enum(literals) = &format {
            writeln!(
                self.header,
                "$comment vcdeez enum {} {} $end",
                token(name),
                literals
                    .iter()
                    .map(|x| token(x))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
        self.leaves.push(Leaf {
            id,
            archives: archives.to_vec(),
            format,
        });
        Ok(())
    }
}
#[derive(Debug)]
struct Event {
    time: u64,
    delta: u32,
    archive: u32,
    value: u64,
}
fn events(data: &[u8], archive_count: u32) -> Result<Vec<Event>, String> {
    let mut r = Reader { data, pos: 0 };
    let mut time = 0u64;
    let mut delta = 0;
    let mut archive = 0i64;
    let mut out = Vec::new();
    while r.pos < data.len() {
        let start = r.pos;
        let op = r.byte()?;
        match op {
            28 => {
                time = r.uint(8)?;
                delta = 0;
            }
            27 | 86 | 87 => {
                time = time
                    .checked_add(r.uint(match op {
                        27 => 1,
                        86 => 2,
                        _ => 4,
                    })?)
                    .ok_or("WLF timestamp overflow")?;
                delta = 0;
            }
            29 | 30 => {
                delta = r.uint(if op == 29 { 1 } else { 4 })? as u32;
            }
            84 => archive = 0,
            85 => {}
            26 => {
                if r.pos != data.len() {
                    return Err("unexpected data after WLF EOF".into());
                }
                return Ok(out);
            }
            0..=23 | 31..=54 => {
                let relative = op >= 31;
                let idx = if relative { op - 31 } else { op };
                let n = [1, 2, 4][(idx % 12 / 4) as usize];
                let raw = r.uint(n)?;
                let offset = if relative && raw & (1 << (n * 8 - 1)) != 0 {
                    raw as i64 - (1i64 << (n * 8))
                } else {
                    raw as i64
                };
                archive = if relative { archive + offset } else { offset };
                let n = [0, 1, 4, 8][(idx % 4) as usize];
                if n == 0 {
                    return Err(format!("unsupported valueless WLF event at {start}"));
                }
                let value = r.uint(n)?;
                if archive <= 0 || archive > i64::from(archive_count) {
                    return Err(format!("WLF archive {archive} out of range at {start}"));
                }
                out.push(Event {
                    time,
                    delta,
                    archive: archive as u32,
                    value,
                });
            }
            _ => {
                return Err(format!(
                    "unsupported WLF event opcode {op:02x} at byte {start}"
                ))
            }
        }
    }
    Err("missing WLF event EOF".into())
}
fn value(leaf: &Leaf, values: &BTreeMap<u32, u64>) -> Result<String, String> {
    match &leaf.format {
        Format::Bits(literals) => {
            let mut bits = String::new();
            for id in &leaf.archives {
                let c = if let Some(v) = values.get(id) {
                    let literal = literals
                        .get(*v as usize)
                        .ok_or_else(|| format!("enum value {v} out of bounds"))?;
                    match literal.trim_matches('\'') {
                        "0" | "L" | "l" => '0',
                        "1" | "H" | "h" => '1',
                        "Z" | "z" => 'z',
                        _ => 'x',
                    }
                } else {
                    'x'
                };
                bits.push(c);
            }
            Ok(if bits.len() == 1 {
                format!("{bits}{}\n", leaf.id)
            } else {
                format!("b{bits} {}\n", leaf.id)
            })
        }
        Format::Integer => Ok(format!(
            "b{:064b} {}\n",
            values.get(&leaf.archives[0]).copied().unwrap_or(0) as u32 as i32 as i64 as u64,
            leaf.id
        )),
        Format::Enum(literals) => {
            let v = *values.get(&leaf.archives[0]).unwrap_or(&0);
            let s = literals
                .get(v as usize)
                .ok_or_else(|| format!("enum value {v} out of bounds"))?;
            Ok(format!("s{} {}\n", token(s), leaf.id))
        }
    }
}
pub fn export(raw: &[u8]) -> Result<String, String> {
    let (_, _, resolution) = crate::native::header(raw)?;
    let blocks = crate::native::single_parcel_blocks(raw)?;
    let contexts: Vec<_> = blocks
        .iter()
        .filter(|(_, b)| matches!(b.first(), Some(0x3d | 8)))
        .collect();
    let streams: Vec<_> = blocks
        .iter()
        .filter(|(_, b)| b.first() == Some(&28))
        .collect();
    if contexts.len() != 1 || streams.len() != 1 {
        return Err("native export currently needs one context block and one event block".into());
    }
    let context = context(&contexts[0].1)?;
    let archive_count = u32::from_be_bytes(
        raw.get(0x1e..0x22)
            .ok_or("truncated WLF header")?
            .try_into()
            .unwrap(),
    );
    let events = events(&streams[0].1, archive_count)?;
    for node in &context.nodes {
        if let Node::Signal { archives, .. } = node {
            if archives.iter().any(|a| *a == 0 || *a > archive_count) {
                return Err(
                    "WLF declaration references an archive outside the header range".into(),
                );
            }
        }
    }
    let timescale = match resolution {
        0 => "1fs",
        1 => "10fs",
        2 => "100fs",
        3 => "1ps",
        4 => "10ps",
        5 => "100ps",
        6 => "1ns",
        7 => "10ns",
        8 => "100ns",
        9 => "1us",
        10 => "10us",
        11 => "100us",
        12 => "1ms",
        13 => "10ms",
        14 => "100ms",
        15 => "1s",
        _ => return Err(format!("unsupported WLF resolution {resolution}")),
    };
    let mut b = Builder {
        types: &context.types,
        header: format!(
            "$version vcdeez native Rust WLF decoder $end\n$timescale {timescale} $end\n"
        ),
        leaves: Vec::new(),
        records: 0,
    };
    let mut root = Vec::new();
    for node in context.nodes {
        match node {
            Node::Begin(name) => {
                let hidden = root.is_empty() && name == "/";
                root.push(hidden);
                if !hidden {
                    writeln!(b.header, "$scope module {} $end", token(&name)).unwrap();
                }
            }
            Node::End => {
                if !root.pop().ok_or("unbalanced context")? {
                    b.header.push_str("$upscope $end\n")
                }
            }
            Node::Signal { name, ty, archives } => b.signal(&name, ty, &archives, 0)?,
        }
    }
    b.header.push_str("$enddefinitions $end\n");
    let mut subscriptions: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, leaf) in b.leaves.iter().enumerate() {
        for archive in &leaf.archives {
            subscriptions.entry(*archive).or_default().push(i);
        }
    }
    let mut values = BTreeMap::new();
    let mut emitted = 0;
    let mut last_time = None;
    let mut last_delta = None;
    for e in &events {
        values.insert(e.archive, e.value);
        if last_time != Some(e.time) {
            writeln!(b.header, "#{}", e.time).unwrap();
            last_time = Some(e.time);
            last_delta = None;
        }
        if last_delta != Some(e.delta) {
            writeln!(b.header, "$comment vcdeez delta {} $end", e.delta).unwrap();
            last_delta = Some(e.delta);
        }
        if let Some(leaves) = subscriptions.get(&e.archive) {
            for i in leaves {
                b.header.push_str(&value(&b.leaves[*i], &values)?);
                emitted += 1;
            }
        }
    }
    eprintln!(
        "Native export: {} signals, {} record scopes, {emitted} VCD value changes",
        b.leaves.len(),
        b.records
    );
    Ok(b.header)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_sample_matches_independent_reference_and_restores() {
        // Digest produced from the prior SDK export by analysis/compare_vcd.py's
        // normalization: sorted full names and each signal's distinct values,
        // preserving order and timestamps, including transitions at one time.
        let raw = include_bytes!("../vsim.wlf");
        let vcd = export(raw).unwrap();
        let mut scopes = Vec::new();
        let mut declarations = BTreeMap::new();
        let mut traces: BTreeMap<String, Vec<(u64, String)>> = BTreeMap::new();
        let mut body = false;
        let mut time = 0;
        let mut count = 0;
        for line in vcd.lines() {
            let words: Vec<_> = line.split_whitespace().collect();
            if words.is_empty() {
                continue;
            }
            if !body {
                match words[0] {
                    "$scope" => scopes.push(words[2]),
                    "$upscope" => {
                        scopes.pop().unwrap();
                    }
                    "$var" => {
                        let mut path = scopes.clone();
                        path.push(words[4]);
                        let name = path.join("/");
                        declarations.insert(
                            words[3],
                            (name.clone(), words[2].parse::<usize>().unwrap(), words[1]),
                        );
                        assert!(traces.insert(name, Vec::new()).is_none());
                    }
                    "$enddefinitions" => body = true,
                    _ => {}
                }
                continue;
            }
            if let Some(t) = line.strip_prefix('#') {
                time = t.parse().unwrap();
                continue;
            }
            if line.starts_with('$') {
                continue;
            }
            let (mut value, id) = if matches!(line.as_bytes()[0], b'b' | b's') {
                (words[0][1..].to_string(), words[1])
            } else {
                (line[..1].to_string(), &line[1..])
            };
            let (name, width, kind) = &declarations[id];
            if matches!(*kind, "wire" | "integer") {
                let fill = if value.starts_with(['x', 'z']) {
                    value.as_bytes()[0] as char
                } else {
                    '0'
                };
                value = format!("{}{value}", fill.to_string().repeat(width - value.len()));
            }
            count += 1;
            let trace = traces.get_mut(name).unwrap();
            if trace.last().is_none_or(|(_, last)| *last != value) {
                trace.push((time, value));
            }
        }
        assert_eq!(traces.len(), 189);
        assert_eq!(count, 7149);
        let mut canonical = String::new();
        for (name, trace) in traces {
            writeln!(canonical, "{name}").unwrap();
            for (time, value) in trace {
                writeln!(canonical, "{time} {value}").unwrap();
            }
        }
        assert_eq!(
            crate::hash::sha256(canonical.as_bytes()),
            "afdf41210401de8b487889ca48812900c76c96012a74fa6463ca6f679b269d7d"
        );
        assert_eq!(
            crate::capsule::restore(&crate::capsule::embed(&vcd, raw)).unwrap(),
            raw
        );
    }
    #[test]
    fn signed_archive_offsets_and_integer_values() {
        // Set archive 2 to 3, then relative -1 sets archive 1 to 2.
        let decoded = events(&[13, 2, 3, 32, 255, 2, 26], 2).unwrap();
        assert_eq!(decoded[1].archive, 1);
        assert_eq!(decoded[1].value, 2);
        let leaf = Leaf {
            id: "!".into(),
            archives: vec![1],
            format: Format::Integer,
        };
        assert_eq!(
            value(&leaf, &BTreeMap::from([(1, u32::MAX as u64)])).unwrap(),
            format!("b{} !\n", "1".repeat(64))
        );
        assert!(events(&[69, 26], 2).is_err());
        assert!(context(&[255]).is_err());
    }
    #[test]
    fn sample_context_and_events() {
        let raw = include_bytes!("../vsim.wlf");
        let blocks = crate::native::single_parcel_blocks(raw).unwrap();
        let c = context(&blocks[1].1).unwrap();
        assert_eq!(c.types.len(), 28);
        assert_eq!(
            c.nodes
                .iter()
                .filter(|n| matches!(n, Node::Signal { .. }))
                .count(),
            144
        );
        let e = events(&blocks[3].1, 580).unwrap();
        assert_eq!(e.len(), 2035);
        assert_eq!(e.last().unwrap().time, 1406250);
        assert_eq!(e.last().unwrap().delta, 2);
        assert!(events(&blocks[3].1[..100], 580).is_err());
    }
}
