use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpType {
    Add,
    Sub,
    Xor,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Operation {
    target: char,
    left: char,
    op: OpType,
    right: char,
}

/// Known Battle.net CheckRevision MPQ seed table from BNCSutil / GHost++.
/// Used to XOR the initial seed 'A' value before hashing file streams.
const MPQ_SEEDS: [u32; 8] = [
    0xE7F4_CB62,
    0xF6A1_4FFC,
    0xAA55_04AF,
    0x871F_CDC2,
    0x11BF_6A18,
    0xC572_92E6,
    0x7927_D27E,
    0x2FEC_8733,
];

/// Evaluates a Battle.net CheckRevision formula string across three game binaries
/// (typically warcraft.exe, Storm.dll, and game.dll).
///
/// Returns the final checksum (register 'C') as `u32`.
pub fn check_revision_flat(
    formula: &str,
    file1: &Path,
    file2: &Path,
    file3: &Path,
    mpq_number: i32,
) -> Result<u32, Error> {
    let (mut a, mut b, mut c, ops) = parse_formula(formula)?;

    let seed = if mpq_number >= 0 && (mpq_number as usize) < MPQ_SEEDS.len() {
        MPQ_SEEDS[mpq_number as usize]
    } else {
        MPQ_SEEDS[1] // Default for standard IX86ver1.mpq
    };

    a ^= seed;

    let files = [file1, file2, file3];
    for path in &files {
        hash_file(path, &ops, &mut a, &mut b, &mut c)?;
    }

    Ok(c)
}

fn parse_formula(formula: &str) -> Result<(u32, u32, u32, Vec<Operation>), Error> {
    let mut a = 0u32;
    let mut b = 0u32;
    let mut c = 0u32;
    let mut ops = Vec::with_capacity(4);

    for token in formula.split_whitespace() {
        if let Some(rest) = token.strip_prefix("A=")
            && let Ok(v) = rest.parse::<u32>()
        {
            a = v;
            continue;
        }
        if let Some(rest) = token.strip_prefix("B=")
            && let Ok(v) = rest.parse::<u32>()
        {
            b = v;
            continue;
        }
        if let Some(rest) = token.strip_prefix("C=")
            && let Ok(v) = rest.parse::<u32>()
        {
            c = v;
            continue;
        }

        // Operation token: 5 chars, e.g. "A=A-S"
        let chars: Vec<char> = token.chars().collect();
        if chars.len() == 5 && chars[1] == '=' {
            let target = chars[0];
            let left = chars[2];
            let op_char = chars[3];
            let right = chars[4];

            let op = match op_char {
                '+' => OpType::Add,
                '-' => OpType::Sub,
                '^' => OpType::Xor,
                '*' => OpType::Mul,
                '/' => OpType::Div,
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        format!("unknown CheckRevision operator '{op_char}'"),
                    ));
                }
            };

            ops.push(Operation {
                target,
                left,
                op,
                right,
            });
        }
    }

    Ok((a, b, c, ops))
}

fn hash_file(
    path: &Path,
    ops: &[Operation],
    a: &mut u32,
    b: &mut u32,
    c: &mut u32,
) -> Result<(), Error> {
    let mut f = File::open(path)?;
    let mut chunk_buf = [0u8; 8192];
    let mut rem_buf = [0u8; 1024];

    loop {
        let n = f.read(&mut chunk_buf)?;
        if n == 0 {
            break;
        }

        let full_blocks = n / 1024;
        let remainder = n % 1024;

        for block in chunk_buf[..full_blocks * 1024].chunks_exact(4) {
            let s = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
            execute_ops(ops, a, b, c, s);
        }

        if remainder > 0 {
            let next_n = f.read(&mut rem_buf[remainder..])?;
            if next_n > 0 {
                rem_buf[..remainder].copy_from_slice(&chunk_buf[full_blocks * 1024..n]);
                let total = remainder + next_n;
                if total == 1024 {
                    for block in rem_buf.chunks_exact(4) {
                        let s = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
                        execute_ops(ops, a, b, c, s);
                    }
                } else {
                    pad_and_execute(ops, a, b, c, &rem_buf[..total]);
                    break;
                }
            } else {
                pad_and_execute(ops, a, b, c, &chunk_buf[full_blocks * 1024..n]);
                break;
            }
        }
    }

    Ok(())
}

#[inline(always)]
fn pad_and_execute(ops: &[Operation], a: &mut u32, b: &mut u32, c: &mut u32, tail: &[u8]) {
    let mut padded = [0u8; 1024];
    let len = tail.len();
    padded[..len].copy_from_slice(tail);

    let mut pad_byte = 0xFFu8;
    for b_dest in &mut padded[len..] {
        *b_dest = pad_byte;
        pad_byte = pad_byte.wrapping_sub(1);
    }

    for block in padded.chunks_exact(4) {
        let s = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        execute_ops(ops, a, b, c, s);
    }
}

#[inline(always)]
fn execute_ops(ops: &[Operation], a: &mut u32, b: &mut u32, c: &mut u32, s: u32) {
    for op in ops {
        let left_val = get_reg(op.left, *a, *b, *c, s);
        let right_val = get_reg(op.right, *a, *b, *c, s);
        let res = match op.op {
            OpType::Add => left_val.wrapping_add(right_val),
            OpType::Sub => left_val.wrapping_sub(right_val),
            OpType::Xor => left_val ^ right_val,
            OpType::Mul => left_val.wrapping_mul(right_val),
            OpType::Div => left_val.checked_div(right_val).unwrap_or(0),
        };
        match op.target {
            'A' => *a = res,
            'B' => *b = res,
            'C' => *c = res,
            _ => {}
        }
    }
}

#[inline(always)]
fn get_reg(reg: char, a: u32, b: u32, c: u32, s: u32) -> u32 {
    match reg {
        'A' => a,
        'B' => b,
        'C' => c,
        'S' => s,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_formula_and_extracts_tokens() {
        let formula = "A=3845581634 B=880823580 C=1363937103 4 A=A-S B=B-C C=C-A A=A-B";
        let (a, b, c, ops) = parse_formula(formula).expect("formula parsed");
        assert_eq!(a, 3845581634);
        assert_eq!(b, 880823580);
        assert_eq!(c, 1363937103);
        assert_eq!(ops.len(), 4);
        assert_eq!(
            ops[0],
            Operation {
                target: 'A',
                left: 'A',
                op: OpType::Sub,
                right: 'S'
            }
        );
        assert_eq!(
            ops[1],
            Operation {
                target: 'B',
                left: 'B',
                op: OpType::Sub,
                right: 'C'
            }
        );
        assert_eq!(
            ops[2],
            Operation {
                target: 'C',
                left: 'C',
                op: OpType::Sub,
                right: 'A'
            }
        );
        assert_eq!(
            ops[3],
            Operation {
                target: 'A',
                left: 'A',
                op: OpType::Sub,
                right: 'B'
            }
        );
    }

    #[test]
    fn evaluates_check_revision_over_test_files() {
        let temp = std::env::temp_dir();
        let f1 = temp.join("cr_test_warcraft.exe");
        let f2 = temp.join("cr_test_storm.dll");
        let f3 = temp.join("cr_test_game.dll");

        std::fs::File::create(&f1)
            .unwrap()
            .write_all(b"Warcraft 3 executable test buffer 1234567890")
            .unwrap();
        std::fs::File::create(&f2)
            .unwrap()
            .write_all(b"Storm.dll library test buffer 1234567890")
            .unwrap();
        std::fs::File::create(&f3)
            .unwrap()
            .write_all(b"Game.dll library test buffer 1234567890")
            .unwrap();

        let formula = "A=3845581634 B=880823580 C=1363937103 4 A=A-S B=B-C C=C-A A=A-B";
        let checksum = check_revision_flat(formula, &f1, &f2, &f3, 1).expect("checksum computed");
        assert_eq!(
            checksum, 2_297_190_262,
            "checksum drifted from reference"
        );

        let _ = std::fs::remove_file(&f1);
        let _ = std::fs::remove_file(&f2);
        let _ = std::fs::remove_file(&f3);
    }
}
