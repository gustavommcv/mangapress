//! Huffman tables built from the picture, which is what libjpeg's `optimize_coding` does and
//! Pillow's `optimize=1` asks for: the picture is coded once with the standard tables, the
//! symbols are counted, tables that suit the count are built (ITU T.81, Annex K.2), and the same
//! symbols are written again with them. The pixels do not change, and the file is smaller.
//!
//! Only the shape of file the encoder here writes is handled: one baseline frame of 8 bits, one
//! scan with all the components, the four standard tables, no restart intervals. Anything else
//! is left as it is.

/// A table as the file lists it: how many codes there are of each length, and the symbols.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Table {
    counts: [u8; 16],
    symbols: Vec<u8>,
}

/// A table seen from the encoder's side: the code and length of each symbol.
struct Codes {
    code: [u16; 256],
    length: [u8; 256],
}

impl Table {
    fn codes(&self) -> Codes {
        let mut codes = Codes {
            code: [0; 256],
            length: [0; 256],
        };
        let (mut code, mut next) = (0u16, 0usize);
        for (length, &count) in self.counts.iter().enumerate() {
            for _ in 0..count {
                let symbol = usize::from(self.symbols[next]);
                codes.code[symbol] = code;
                codes.length[symbol] = (length + 1) as u8;
                code += 1;
                next += 1;
            }
            code <<= 1;
        }
        codes
    }

    /// For decoding: the smallest and largest code of each length, and where its symbols start.
    fn decoder(&self) -> Decoder {
        let mut decoder = Decoder {
            min: [0; 17],
            max: [-1; 17],
            first: [0; 17],
            symbols: self.symbols.clone(),
        };
        let (mut code, mut next) = (0i32, 0usize);
        for length in 1..=16 {
            let count = usize::from(self.counts[length - 1]);
            decoder.first[length] = next;
            decoder.min[length] = code;
            if count > 0 {
                decoder.max[length] = code + count as i32 - 1;
            }
            code = (code + count as i32) << 1;
            next += count;
        }
        decoder
    }
}

struct Decoder {
    min: [i32; 17],
    max: [i32; 17],
    first: [usize; 17],
    symbols: Vec<u8>,
}

/// Where a scan's entropy-coded data is read from: the bytes with their stuffing taken out.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    left: u8,
}

impl<'a> Bits<'a> {
    fn bit(&mut self) -> Option<u32> {
        if self.left == 0 {
            self.at += 1;
            self.left = 8;
        }
        let byte = *self.data.get(self.at)?;
        self.left -= 1;
        Some(u32::from(byte >> self.left) & 1)
    }

    fn bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    fn symbol(&mut self, decoder: &Decoder) -> Option<u8> {
        let mut code = 0i32;
        for length in 1..=16 {
            code = (code << 1) | self.bit()? as i32;
            if decoder.max[length] >= 0
                && code <= decoder.max[length]
                && code >= decoder.min[length]
            {
                let index = decoder.first[length] + (code - decoder.min[length]) as usize;
                return decoder.symbols.get(index).copied();
            }
        }
        None
    }
}

/// How the scan's blocks are laid out.
struct Layout {
    /// Per component, in scan order: its sampling factors and the tables it uses
    /// (indexes 0..4 for DC0, AC0, DC1, AC1).
    components: Vec<(usize, usize, usize, usize)>,
    mcus_x: usize,
    mcus_y: usize,
}

/// One coded value: which of the four tables, the symbol, and the bits that follow it.
struct Item {
    table: usize,
    symbol: u8,
    extra: u32,
    extra_length: u8,
}

/// Read every symbol of the scan, in order, handing each to `visit`.
fn walk(
    data: &[u8],
    layout: &Layout,
    decoders: &[Decoder; 4],
    mut visit: impl FnMut(Item),
) -> Option<()> {
    let mut bits = Bits {
        data,
        at: 0,
        left: 8,
    };
    for _ in 0..layout.mcus_y * layout.mcus_x {
        for &(h, v, dc, ac) in &layout.components {
            for _ in 0..h * v {
                let symbol = bits.symbol(&decoders[dc])?;
                if symbol > 11 {
                    return None;
                }
                let extra = if symbol > 0 { bits.bits(symbol)? } else { 0 };
                visit(Item {
                    table: dc,
                    symbol,
                    extra,
                    extra_length: symbol,
                });
                let mut k = 1;
                while k < 64 {
                    let symbol = bits.symbol(&decoders[ac])?;
                    let (run, size) = (usize::from(symbol >> 4), symbol & 15);
                    let extra = if size > 0 { bits.bits(size)? } else { 0 };
                    visit(Item {
                        table: ac,
                        symbol,
                        extra,
                        extra_length: size,
                    });
                    if size == 0 {
                        if run == 15 {
                            k += 16;
                            continue;
                        }
                        break;
                    }
                    k += run + 1;
                }
                if k > 64 {
                    return None;
                }
            }
        }
    }
    Some(())
}

/// T.81 Annex K.2: a table for symbols that occur `counts[symbol]` times. No code is all ones,
/// and none is longer than 16 bits.
fn optimal_table(counts: &[u64; 256]) -> Option<Table> {
    // 257 symbols: the last is a stand-in with a count of 1, so that the longest code is not
    // all ones (which the format reserves).
    let mut frequency = [0u64; 257];
    frequency[..256].copy_from_slice(counts);
    frequency[256] = 1;
    let mut size = [0usize; 257];
    let mut chain = [usize::MAX; 257];

    // The least frequent symbol, the largest of them when several are as rare.
    let least = |frequency: &[u64; 257], skip: Option<usize>| {
        let mut found = None;
        let mut value = u64::MAX;
        for (symbol, &count) in frequency.iter().enumerate() {
            if count > 0 && Some(symbol) != skip && count <= value {
                value = count;
                found = Some(symbol);
            }
        }
        found
    };
    while let Some(first) = least(&frequency, None) {
        let Some(second) = least(&frequency, Some(first)) else {
            break;
        };
        frequency[first] += frequency[second];
        frequency[second] = 0;
        let mut at = first;
        loop {
            size[at] += 1;
            if chain[at] == usize::MAX {
                break;
            }
            at = chain[at];
        }
        chain[at] = second;
        let mut at = second;
        loop {
            size[at] += 1;
            if chain[at] == usize::MAX {
                break;
            }
            at = chain[at];
        }
    }

    // How many codes there are of each length. Over 32 bits is not something a picture produces.
    let mut count = [0u32; 33];
    for &length in size.iter().filter(|&&length| length > 0) {
        if length > 32 {
            return None;
        }
        count[length] += 1;
    }
    // Shorten any code over 16 bits (Figure K.3).
    for i in (17..=32).rev() {
        while count[i] > 0 {
            let mut j = i - 2;
            while count[j] == 0 {
                j -= 1;
            }
            count[i] -= 2;
            count[i - 1] += 1;
            count[j + 1] += 2;
            count[j] -= 1;
        }
    }
    // Take the stand-in out: it has the longest code.
    let mut i = 16;
    while count[i] == 0 {
        i -= 1;
    }
    count[i] -= 1;

    let mut table = Table {
        counts: [0; 16],
        symbols: Vec::new(),
    };
    for (slot, &codes) in table.counts.iter_mut().zip(&count[1..=16]) {
        *slot = codes as u8;
    }
    // The symbols, shortest code first, in symbol order within a length.
    for length in 1..=32 {
        table.symbols.extend(
            size[..256]
                .iter()
                .enumerate()
                .filter(|&(_, &symbol_size)| symbol_size == length)
                .map(|(symbol, _)| symbol as u8),
        );
    }
    Some(table)
}

struct Writer {
    bytes: Vec<u8>,
    accumulator: u32,
    held: u8,
}

impl Writer {
    fn put(&mut self, value: u32, length: u8) {
        if length == 0 {
            return;
        }
        self.accumulator = (self.accumulator << length) | (value & ((1 << length) - 1));
        self.held += length;
        while self.held >= 8 {
            self.held -= 8;
            let byte = (self.accumulator >> self.held) as u8;
            self.bytes.push(byte);
            if byte == 0xFF {
                self.bytes.push(0);
            }
        }
        self.accumulator &= (1 << self.held) - 1;
    }

    fn finish(mut self) -> Vec<u8> {
        if self.held > 0 {
            let pad = 8 - self.held;
            self.put((1 << pad) - 1, pad);
        }
        self.bytes
    }
}

/// A segment of the file: its marker and what follows the length.
struct Segment<'a> {
    marker: u8,
    start: usize,
    end: usize,
    body: &'a [u8],
}

fn segments(bytes: &[u8]) -> Option<(Vec<Segment<'_>>, usize)> {
    if bytes.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut found = Vec::new();
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(at + 2)?,
            *bytes.get(at + 3)?,
        ]));
        let end = at + 2 + length;
        found.push(Segment {
            marker,
            start: at,
            end,
            body: bytes.get(at + 4..end)?,
        });
        if marker == 0xDA {
            return Some((found, end));
        }
        at = end;
    }
}

fn parse_tables(body: &[u8]) -> Option<Vec<(usize, Table)>> {
    let mut tables = Vec::new();
    let mut at = 0;
    while at < body.len() {
        let (class, number) = (body[at] >> 4, usize::from(body[at] & 0x0F));
        let counts: [u8; 16] = body.get(at + 1..at + 17)?.try_into().ok()?;
        let total: usize = counts.iter().map(|&n| usize::from(n)).sum();
        let symbols = body.get(at + 17..at + 17 + total)?.to_vec();
        if number > 1 || class > 1 {
            return None;
        }
        // Index: DC0, AC0, DC1, AC1.
        tables.push((number * 2 + usize::from(class), Table { counts, symbols }));
        at += 17 + total;
    }
    Some(tables)
}

/// The same picture with Huffman tables built from it, or `None` when the file is not of the
/// shape described above (or when nothing is gained).
pub(super) fn optimize(bytes: &[u8]) -> Option<Vec<u8>> {
    let (segments, scan_start) = segments(bytes)?;

    let mut tables: [Option<Table>; 4] = [None, None, None, None];
    let mut frame = None;
    let mut scan = None;
    for segment in &segments {
        match segment.marker {
            0xC0 => frame = Some(segment),
            0xC1..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None, // not baseline
            0xC4 => {
                for (index, table) in parse_tables(segment.body)? {
                    tables[index] = Some(table);
                }
            }
            0xDD => return None, // restart intervals
            0xDA => scan = Some(segment),
            _ => {}
        }
    }
    let frame = frame?.body;
    let scan = scan?.body;
    let (height, width) = (
        usize::from(u16::from_be_bytes([frame[1], frame[2]])),
        usize::from(u16::from_be_bytes([frame[3], frame[4]])),
    );
    let count = usize::from(*frame.get(5)?);
    if frame[0] != 8
        || !(count == 1 || count == 3)
        || scan[0] != count as u8
        || width == 0
        || height == 0
    {
        return None;
    }
    let mut sampling = Vec::new();
    for n in 0..count {
        let component = frame.get(6 + 3 * n..9 + 3 * n)?;
        sampling.push((
            usize::from(component[1] >> 4),
            usize::from(component[1] & 15),
            component[0],
        ));
    }
    let (h_max, v_max) = (
        sampling.iter().map(|s| s.0).max()?,
        sampling.iter().map(|s| s.1).max()?,
    );
    let mut components = Vec::new();
    for n in 0..count {
        let selector = scan.get(1 + 2 * n..3 + 2 * n)?;
        let &(h, v, _) = sampling.iter().find(|s| s.2 == selector[0])?;
        let (dc, ac) = (usize::from(selector[1] >> 4), usize::from(selector[1] & 15));
        if dc > 1 || ac > 1 {
            return None;
        }
        components.push((h, v, dc * 2, ac * 2 + 1));
    }
    let layout = if count == 1 {
        Layout {
            components: vec![(1, 1, components[0].2, components[0].3)],
            mcus_x: width.div_ceil(8),
            mcus_y: height.div_ceil(8),
        }
    } else {
        Layout {
            components,
            mcus_x: width.div_ceil(8 * h_max),
            mcus_y: height.div_ceil(8 * v_max),
        }
    };

    // The scan's data without its stuffing, up to the marker that ends it.
    let mut data = Vec::new();
    let mut at = scan_start;
    loop {
        let byte = *bytes.get(at)?;
        if byte == 0xFF {
            match *bytes.get(at + 1)? {
                0x00 => {
                    data.push(0xFF);
                    at += 2;
                }
                0xD9 => break,
                _ => return None,
            }
        } else {
            data.push(byte);
            at += 1;
        }
    }
    if bytes.len() != at + 2 {
        return None;
    }

    let decoders = [
        tables[0].as_ref()?.decoder(),
        tables[1].as_ref()?.decoder(),
        tables[2].as_ref().or(tables[0].as_ref())?.decoder(),
        tables[3].as_ref().or(tables[1].as_ref())?.decoder(),
    ];
    let mut counts = [[0u64; 256]; 4];
    walk(&data, &layout, &decoders, |item| {
        counts[item.table][usize::from(item.symbol)] += 1;
    })?;

    // Tables for the ones the scan uses; the others are not written.
    let used = |table: usize| counts[table].iter().any(|&n| n > 0);
    let mut new: [Option<Table>; 4] = [None, None, None, None];
    for (table, slot) in new.iter_mut().enumerate() {
        if used(table) {
            *slot = Some(optimal_table(&counts[table])?);
        }
    }
    let codes: [Option<Codes>; 4] =
        std::array::from_fn(|table| new[table].as_ref().map(Table::codes));

    let mut writer = Writer {
        bytes: Vec::with_capacity(data.len()),
        accumulator: 0,
        held: 0,
    };
    walk(&data, &layout, &decoders, |item| {
        let codes = codes[item.table]
            .as_ref()
            .expect("a table for every symbol that occurs");
        writer.put(
            u32::from(codes.code[usize::from(item.symbol)]),
            codes.length[usize::from(item.symbol)],
        );
        writer.put(item.extra, item.extra_length);
    })?;
    let coded = writer.finish();

    // The file: everything before the first table, the new tables in libjpeg's order
    // (DC0, AC0, DC1, AC1), everything between the old ones and the scan, the scan.
    let mut out = Vec::with_capacity(bytes.len());
    out.extend([0xFF, 0xD8]);
    let mut wrote_tables = false;
    for segment in &segments {
        if segment.marker == 0xC4 {
            if !wrote_tables {
                for (index, table) in new.iter().enumerate() {
                    if let Some(table) = table {
                        let class = (index % 2) as u8;
                        let number = (index / 2) as u8;
                        let length = 2 + 1 + 16 + table.symbols.len();
                        out.extend([0xFF, 0xC4]);
                        out.extend((length as u16).to_be_bytes());
                        out.push((class << 4) | number);
                        out.extend(table.counts);
                        out.extend(&table.symbols);
                    }
                }
                wrote_tables = true;
            }
        } else {
            out.extend(&bytes[segment.start..segment.end]);
        }
    }
    out.extend(&coded);
    out.extend([0xFF, 0xD9]);
    (out.len() < bytes.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_for_two_symbols_gives_each_a_code_of_one_bit_or_two() {
        let mut counts = [0u64; 256];
        counts[3] = 100;
        counts[9] = 5;
        let table = optimal_table(&counts).unwrap();
        // Never all ones: with the stand-in symbol, the two real ones take 1 and 2 bits.
        assert_eq!(table.counts[0], 1);
        assert_eq!(table.counts[1], 1);
        assert_eq!(table.symbols, [3, 9]);
    }

    #[test]
    fn no_code_is_longer_than_sixteen_bits_whatever_the_counts() {
        let mut counts = [0u64; 256];
        // Each symbol twice as common as the one before: the plain tree is 30 bits deep.
        for (symbol, count) in counts.iter_mut().enumerate().take(30) {
            *count = 1u64 << symbol;
        }
        let table = optimal_table(&counts).unwrap();
        assert_eq!(
            table.counts.iter().map(|&n| usize::from(n)).sum::<usize>(),
            30
        );
        assert_eq!(table.symbols.len(), 30);
        // The codes fit with room to spare: the code of all ones is never given.
        let used: u64 = table
            .counts
            .iter()
            .enumerate()
            .map(|(length, &n)| u64::from(n) << (15 - length))
            .sum();
        assert!(used < 1u64 << 16, "the lengths overfill the code space");
    }

    #[test]
    fn a_decoder_reads_back_the_codes_the_encoder_writes() {
        let mut counts = [0u64; 256];
        for (symbol, count) in [(0, 50u64), (1, 30), (2, 12), (17, 6), (240, 2)] {
            counts[symbol] = count;
        }
        let table = optimal_table(&counts).unwrap();
        let codes = table.codes();
        let decoder = table.decoder();
        let mut writer = Writer {
            bytes: Vec::new(),
            accumulator: 0,
            held: 0,
        };
        let sent = [0u8, 1, 2, 0, 17, 240, 1, 0, 0];
        for &symbol in &sent {
            writer.put(
                u32::from(codes.code[usize::from(symbol)]),
                codes.length[usize::from(symbol)],
            );
        }
        let bytes = writer.finish();
        let mut bits = Bits {
            data: &bytes,
            at: 0,
            left: 8,
        };
        for &symbol in &sent {
            assert_eq!(bits.symbol(&decoder), Some(symbol));
        }
    }

    #[test]
    fn stuffing_is_written_after_a_byte_of_ones() {
        let mut writer = Writer {
            bytes: Vec::new(),
            accumulator: 0,
            held: 0,
        };
        writer.put(0xFF, 8);
        writer.put(0b101, 3);
        assert_eq!(writer.finish(), [0xFF, 0x00, 0b1011_1111]);
    }
}
