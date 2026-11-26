//! LLVM bitstream primitives.
//!
//! The bitstream is a sequence of 32-bit little-endian words read least
//! significant bit first. Records are either unabbreviated (a fixed layout) or
//! described by a per-block abbreviation.

use llvm_rust_support::{Error, Result};

/// Raw bitcode magic, stored as the first little-endian word.
pub const MAGIC: u32 = 0xDEC0_4342;

pub const END_BLOCK: u32 = 0;
pub const ENTER_SUBBLOCK: u32 = 1;
pub const DEFINE_ABBREV: u32 = 2;
pub const UNABBREV_RECORD: u32 = 3;
pub const FIRST_APPLICATION_ABBREV: u32 = 4;

pub const BLOCK_ID_WIDTH: u32 = 8;
pub const CODE_LEN_WIDTH: u32 = 4;
pub const BLOCK_SIZE_WIDTH: u32 = 32;

/// One operand description in an abbreviation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbbrevOp {
    Literal(u64),
    Fixed(u32),
    Vbr(u32),
    Array(Box<AbbrevOp>),
    Char6,
    Blob,
}

#[derive(Debug)]
struct BlockFrame {
    prev_code_len: u32,
    size_word: usize,
    prev_abbrevs: Vec<Vec<AbbrevOp>>,
}

/// Bit-level writer producing a complete bitcode stream.
#[derive(Debug)]
pub struct BitWriter {
    words: Vec<u32>,
    cur: u32,
    bit: u32,
    code_len: u32,
    blocks: Vec<BlockFrame>,
    abbrevs: Vec<Vec<AbbrevOp>>,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            words: vec![MAGIC],
            cur: 0,
            bit: 0,
            code_len: 2,
            blocks: Vec::new(),
            abbrevs: Vec::new(),
        }
    }

    pub fn write_bits(&mut self, value: u64, bits: u32) {
        debug_assert!(bits > 0 && bits <= 32);
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        let value = (value as u32) & mask;
        self.cur |= value << self.bit;
        if self.bit + bits >= 32 {
            self.words.push(self.cur);
            self.cur = if self.bit == 0 {
                0
            } else {
                value >> (32 - self.bit)
            };
            self.bit = (self.bit + bits) & 31;
        } else {
            self.bit += bits;
        }
    }

    pub fn align32(&mut self) {
        if self.bit != 0 {
            self.words.push(self.cur);
            self.cur = 0;
            self.bit = 0;
        }
    }

    pub fn write_vbr(&mut self, mut value: u64, chunk_bits: u32) {
        debug_assert!(chunk_bits > 1 && chunk_bits <= 32);
        let data_bits = chunk_bits - 1;
        let threshold = 1u64 << data_bits;
        while value >= threshold {
            self.write_bits((value & (threshold - 1)) | threshold, chunk_bits);
            value >>= data_bits;
        }
        self.write_bits(value, chunk_bits);
    }

    pub fn write_code(&mut self, value: u32) {
        self.write_bits(value as u64, self.code_len);
    }

    pub fn enter_subblock(&mut self, block_id: u32, code_len: u32) {
        self.write_code(ENTER_SUBBLOCK);
        self.write_vbr(block_id as u64, BLOCK_ID_WIDTH);
        self.write_vbr(code_len as u64, CODE_LEN_WIDTH);
        self.align32();
        let size_word = self.words.len();
        self.words.push(0);
        let prev_code_len = self.code_len;
        let prev_abbrevs = core::mem::take(&mut self.abbrevs);
        self.blocks.push(BlockFrame {
            prev_code_len,
            size_word,
            prev_abbrevs,
        });
        self.code_len = code_len;
    }

    pub fn exit_block(&mut self) {
        self.write_code(END_BLOCK);
        self.align32();
        let frame = self.blocks.pop().expect("unbalanced bitstream blocks");
        let size = (self.words.len() - frame.size_word - 1) as u32;
        self.words[frame.size_word] = size;
        self.code_len = frame.prev_code_len;
        self.abbrevs = frame.prev_abbrevs;
    }

    pub fn define_abbrev(&mut self, ops: &[AbbrevOp]) -> u32 {
        self.write_code(DEFINE_ABBREV);
        self.write_vbr(ops.len() as u64, 5);
        for op in ops {
            match op {
                AbbrevOp::Literal(value) => {
                    self.write_bits(1, 1);
                    self.write_vbr(*value, 8);
                }
                AbbrevOp::Blob => {
                    self.write_bits(0, 1);
                    self.write_bits(5, 3);
                }
                _ => unreachable!("writer only defines literal and blob abbreviations"),
            }
        }
        self.abbrevs.push(ops.to_vec());
        FIRST_APPLICATION_ABBREV + (self.abbrevs.len() as u32 - 1)
    }

    pub fn emit_record(&mut self, code: u32, operands: &[u64]) {
        self.write_code(UNABBREV_RECORD);
        self.write_vbr(code as u64, 6);
        self.write_vbr(operands.len() as u64, 6);
        for operand in operands {
            self.write_vbr(*operand, 6);
        }
    }

    pub fn emit_blob_record(&mut self, abbrev: u32, blob: &[u8]) {
        self.write_code(abbrev);
        self.write_vbr(blob.len() as u64, 6);
        self.align32();
        for byte in blob {
            self.write_bits(*byte as u64, 8);
        }
        self.align32();
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.align32();
        debug_assert!(self.blocks.is_empty());
        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for word in self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

impl Default for BitWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Bit-level reader over a raw bitcode stream.
#[derive(Debug)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 4 || &bytes[..4] != b"BC\xc0\xde" {
            return Err(Error::bitcode("missing bitcode magic"));
        }
        Ok(Self { bytes, bit: 32 })
    }

    pub fn at_end(&self) -> bool {
        self.bit >= self.bytes.len() * 8
    }

    pub fn read_bits(&mut self, bits: u32) -> Result<u64> {
        if bits == 0 || bits > 32 {
            return Err(Error::bitcode("invalid bit width"));
        }
        if self.bit + bits as usize > self.bytes.len() * 8 {
            return Err(Error::bitcode("unexpected end of bitstream"));
        }
        let mut value = 0u64;
        for i in 0..bits {
            let byte = self.bytes[(self.bit + i as usize) / 8];
            let bit = (byte >> ((self.bit + i as usize) % 8)) & 1;
            value |= (bit as u64) << i;
        }
        self.bit += bits as usize;
        Ok(value)
    }

    pub fn read_vbr(&mut self, chunk_bits: u32) -> Result<u64> {
        let data_bits = chunk_bits - 1;
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let chunk = self.read_bits(chunk_bits)?;
            value |= (chunk & ((1u64 << data_bits) - 1)) << shift;
            if chunk & (1u64 << data_bits) == 0 {
                return Ok(value);
            }
            shift += data_bits;
            if shift > 64 {
                return Err(Error::bitcode("VBR value too large"));
            }
        }
    }

    pub fn align32(&mut self) {
        self.bit = (self.bit + 31) & !31;
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        self.align32();
        if self.bit + 32 > self.bytes.len() * 8 {
            return Err(Error::bitcode("unexpected end of bitstream"));
        }
        let start = self.bit / 8;
        let word = u32::from_le_bytes([
            self.bytes[start],
            self.bytes[start + 1],
            self.bytes[start + 2],
            self.bytes[start + 3],
        ]);
        self.bit += 32;
        Ok(word)
    }
}

/// One decoded record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub code: u64,
    pub operands: Vec<u64>,
    pub blob: Option<Vec<u8>>,
}

/// A block item in stream order.
#[derive(Clone, Debug)]
pub enum Item {
    Record(Record),
    Block(Block),
}

/// A decoded block.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: u32,
    pub code_len: u32,
    pub items: Vec<Item>,
}

impl Block {
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.items.iter().filter_map(|item| match item {
            Item::Record(record) => Some(record),
            Item::Block(_) => None,
        })
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.items.iter().filter_map(|item| match item {
            Item::Block(block) => Some(block),
            Item::Record(_) => None,
        })
    }
}

#[derive(Clone, Debug)]
struct Abbrev {
    ops: Vec<AbbrevOp>,
}

/// A fully decoded bitcode file.
#[derive(Clone, Debug)]
pub struct Bitstream {
    pub blocks: Vec<Block>,
}

impl Bitstream {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = BitReader::new(bytes)?;
        let mut blockinfo: std::collections::HashMap<u32, Vec<Abbrev>> =
            std::collections::HashMap::new();
        let mut blocks = Vec::new();
        while !reader.at_end() {
            let abbrev = reader.read_bits(2)?;
            match abbrev {
                1 => {
                    let block = parse_block(&mut reader, &mut blockinfo, false)?;
                    blocks.push(block);
                }
                0 => reader.align32(),
                3 => {
                    let _ = read_unabbreviated(&mut reader)?;
                }
                _ => return Err(Error::bitcode("unexpected top-level bitstream record")),
            }
        }
        Ok(Self { blocks })
    }
}

fn parse_block(
    reader: &mut BitReader<'_>,
    blockinfo: &mut std::collections::HashMap<u32, Vec<Abbrev>>,
    _is_blockinfo: bool,
) -> Result<Block> {
    let id = reader.read_vbr(BLOCK_ID_WIDTH)? as u32;
    let is_blockinfo = id == 0;
    let code_len = reader.read_vbr(CODE_LEN_WIDTH)? as u32;
    reader.align32();
    let _size = reader.read_u32()?;
    let mut abbrevs = if is_blockinfo {
        Vec::new()
    } else {
        blockinfo.get(&id).cloned().unwrap_or_default()
    };
    let mut items = Vec::new();
    let mut current_blockinfo_id = None;
    loop {
        let abbrev = reader.read_bits(code_len)?;
        match abbrev {
            0 => {
                reader.align32();
                break;
            }
            1 => {
                let child = parse_block(reader, blockinfo, false)?;
                items.push(Item::Block(child));
            }
            2 => {
                let abbrev = read_abbrev(reader)?;
                if is_blockinfo {
                    if let Some(bid) = current_blockinfo_id {
                        blockinfo.entry(bid).or_default().push(abbrev);
                    }
                } else {
                    abbrevs.push(abbrev);
                }
            }
            3 => {
                let record = read_unabbreviated(reader)?;
                if is_blockinfo && record.code == 1 && !record.operands.is_empty() {
                    current_blockinfo_id = Some(record.operands[0] as u32);
                }
                items.push(Item::Record(record));
            }
            other => {
                let index = (other as usize)
                    .checked_sub(FIRST_APPLICATION_ABBREV as usize)
                    .ok_or_else(|| Error::bitcode("invalid abbreviation id"))?;
                let abbrev = abbrevs
                    .get(index)
                    .ok_or_else(|| Error::bitcode("unknown abbreviation"))?
                    .clone();
                let record = read_abbreviated(reader, &abbrev)?;
                items.push(Item::Record(record));
            }
        }
    }
    Ok(Block {
        id,
        code_len,
        items,
    })
}

fn read_unabbreviated(reader: &mut BitReader<'_>) -> Result<Record> {
    let code = reader.read_vbr(6)?;
    let count = reader.read_vbr(6)? as usize;
    let mut operands = Vec::with_capacity(count);
    for _ in 0..count {
        operands.push(reader.read_vbr(6)?);
    }
    Ok(Record {
        code,
        operands,
        blob: None,
    })
}

fn read_abbrev(reader: &mut BitReader<'_>) -> Result<Abbrev> {
    let count = reader.read_vbr(5)? as usize;
    let mut flat = Vec::with_capacity(count);
    for _ in 0..count {
        let literal = reader.read_bits(1)? != 0;
        if literal {
            flat.push(AbbrevOp::Literal(reader.read_vbr(8)?));
        } else {
            let encoding = reader.read_bits(3)?;
            flat.push(match encoding {
                1 => AbbrevOp::Fixed(reader.read_vbr(5)? as u32),
                2 => AbbrevOp::Vbr(reader.read_vbr(5)? as u32),
                3 => AbbrevOp::Array(Box::new(AbbrevOp::Fixed(0))),
                4 => AbbrevOp::Char6,
                5 => AbbrevOp::Blob,
                _ => return Err(Error::bitcode("invalid abbreviation encoding")),
            });
        }
    }
    let mut ops = Vec::with_capacity(flat.len());
    let mut index = 0;
    while index < flat.len() {
        if matches!(flat[index], AbbrevOp::Array(_)) {
            let element = flat
                .get(index + 1)
                .cloned()
                .ok_or_else(|| Error::bitcode("array abbreviation without element"))?;
            ops.push(AbbrevOp::Array(Box::new(element)));
            index += 2;
        } else {
            ops.push(flat[index].clone());
            index += 1;
        }
    }
    Ok(Abbrev { ops })
}

fn read_abbreviated(reader: &mut BitReader<'_>, abbrev: &Abbrev) -> Result<Record> {
    let mut ops = abbrev.ops.iter();
    let first = ops
        .next()
        .ok_or_else(|| Error::bitcode("empty abbreviation"))?;
    let code = match first {
        AbbrevOp::Literal(value) => *value,
        other => decode_field(reader, other)?,
    };
    let mut operands = Vec::new();
    let mut blob = None;
    for op in ops {
        match op {
            AbbrevOp::Literal(value) => operands.push(*value),
            AbbrevOp::Array(element) => {
                let count = reader.read_vbr(6)? as usize;
                for _ in 0..count {
                    operands.push(decode_field(reader, element)?);
                }
            }
            AbbrevOp::Blob => {
                let len = reader.read_vbr(6)? as usize;
                reader.align32();
                let mut bytes = Vec::with_capacity(len);
                for _ in 0..len {
                    bytes.push(reader.read_bits(8)? as u8);
                }
                reader.align32();
                blob = Some(bytes);
            }
            other => operands.push(decode_field(reader, other)?),
        }
    }
    Ok(Record {
        code,
        operands,
        blob,
    })
}

fn decode_field(reader: &mut BitReader<'_>, op: &AbbrevOp) -> Result<u64> {
    match op {
        AbbrevOp::Literal(value) => Ok(*value),
        AbbrevOp::Fixed(width) => reader.read_bits(*width),
        AbbrevOp::Vbr(width) => reader.read_vbr(*width),
        AbbrevOp::Char6 => {
            let value = reader.read_bits(6)? as usize;
            Ok(b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._"[value] as u64)
        }
        AbbrevOp::Array(_) | AbbrevOp::Blob => Err(Error::bitcode("invalid scalar abbreviation")),
    }
}
