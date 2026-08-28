use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;
use std::sync::RwLock;

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

pub const DEFAULT_MPQ_SEEDS: [u32; 8] = [
    0xE7F4_CB62,
    0xF6A1_4FFC,
    0xAA55_04AF,
    0x871F_CDC2,
    0x11BF_6A18,
    0xC572_92E6,
    0x7927_D27E,
    0x2FEC_8733,
];

static SEED_REGISTRY: RwLock<Option<Vec<u32>>> = RwLock::new(None);

pub fn get_mpq_seed(mpq_number: i32) -> u32 {
    if mpq_number < 0 {
        return 0;
    }
    let idx = mpq_number as usize;
    let guard = SEED_REGISTRY.read().unwrap();
    if let Some(seeds) = &*guard {
        seeds.get(idx).copied().unwrap_or(0)
    } else {
        DEFAULT_MPQ_SEEDS.get(idx).copied().unwrap_or(0)
    }
}

pub fn set_mpq_seed(mpq_number: i32, new_seed: u32) -> u32 {
    if mpq_number < 0 {
        return 0;
    }
    let idx = mpq_number as usize;
    let mut guard = SEED_REGISTRY.write().unwrap();
    let seeds = guard.get_or_insert_with(|| DEFAULT_MPQ_SEEDS.to_vec());
    if idx >= seeds.len() {
        seeds.resize(idx + 1, 0);
    }
    let old = seeds[idx];
    seeds[idx] = new_seed;
    old
}

pub fn check_revision(formula: &str, files: &[&Path], mpq_number: i32) -> Result<u32, Error> {
    if files.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "checkRevision requires at least one file",
        ));
    }

    let (mut a, mut b, mut c, ops) = parse_formula(formula)?;
    let seed = get_mpq_seed(mpq_number);
    let effective_seed = if seed != 0 {
        seed
    } else {
        DEFAULT_MPQ_SEEDS[1]
    };

    a ^= effective_seed;

    for path in files {
        hash_file(path, &ops, &mut a, &mut b, &mut c)?;
    }

    Ok(c)
}

#[inline]
pub fn check_revision_flat(
    formula: &str,
    file1: &Path,
    file2: &Path,
    file3: &Path,
    mpq_number: i32,
) -> Result<u32, Error> {
    check_revision(formula, &[file1, file2, file3], mpq_number)
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
    let mut block = [0u8; 1024];

    loop {
        let mut total = 0;
        while total < 1024 {
            match f.read(&mut block[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }

        if total == 0 {
            break;
        }

        if total == 1024 {
            for chunk in block.chunks_exact(4) {
                let s = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                execute_ops(ops, a, b, c, s);
            }
        } else {
            let mut pad_byte = 0xFFu8;
            for b_dest in &mut block[total..] {
                *b_dest = pad_byte;
                pad_byte = pad_byte.wrapping_sub(1);
            }
            for chunk in block.chunks_exact(4) {
                let s = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                execute_ops(ops, a, b, c, s);
            }
            break;
        }
    }

    Ok(())
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
    }

    #[test]
    fn seed_get_and_set_works() {
        let s1 = get_mpq_seed(1);
        assert_eq!(s1, 0xF6A1_4FFC);
        let old = set_mpq_seed(1, 0x12345678);
        assert_eq!(old, 0xF6A1_4FFC);
        assert_eq!(get_mpq_seed(1), 0x12345678);
        set_mpq_seed(1, 0xF6A1_4FFC);
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
        assert_eq!(checksum, 2_297_190_262, "checksum drifted from reference");

        let _ = std::fs::remove_file(&f1);
        let _ = std::fs::remove_file(&f2);
        let _ = std::fs::remove_file(&f3);
    }
}
