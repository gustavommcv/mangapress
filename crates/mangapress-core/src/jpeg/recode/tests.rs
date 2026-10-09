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
