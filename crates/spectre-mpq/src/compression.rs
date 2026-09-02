use crate::error::MpqError;
use implode::exploder::Exploder;
use implode::symbol::DEFAULT_CODE_TABLE;

#[allow(dead_code)]
pub const COMPRESSION_HUFFMAN: u8 = 0x01;
pub const COMPRESSION_ZLIB: u8 = 0x02;
pub const COMPRESSION_PKWARE: u8 = 0x08;
pub const COMPRESSION_BZIP2: u8 = 0x10;
#[allow(dead_code)]
pub const COMPRESSION_SPARSE: u8 = 0x20;
#[allow(dead_code)]
pub const COMPRESSION_ADPCM_MONO: u8 = 0x40;
#[allow(dead_code)]
pub const COMPRESSION_ADPCM_STEREO: u8 = 0x80;

pub fn decompress_multi(data: &[u8], out: &mut [u8]) -> Result<usize, MpqError> {
    if data.is_empty() {
        return Ok(0);
    }

    let compression_type = data[0];
    let payload = &data[1..];

    if compression_type & COMPRESSION_ZLIB != 0 {
        let mut zlib = flate2::Decompress::new(true);
        match zlib.decompress(payload, out, flate2::FlushDecompress::None) {
            Ok(_) => return Ok(zlib.total_out() as usize),
            Err(e) => {
                return Err(MpqError::DecompressionFailed(format!(
                    "ZLIB decompress failed: {e}"
                )));
            }
        }
    }

    if compression_type & COMPRESSION_PKWARE != 0 {
        return decompress_pkware(payload, out);
    }

    if compression_type & COMPRESSION_BZIP2 != 0 {
        return Err(MpqError::UnsupportedCompression(COMPRESSION_BZIP2));
    }

    Err(MpqError::UnsupportedCompression(compression_type))
}

pub fn decompress_pkware(data: &[u8], out: &mut [u8]) -> Result<usize, MpqError> {
    let mut exploder = Exploder::new(&DEFAULT_CODE_TABLE);
    let mut cpos: usize = 0;
    let mut out_pos: usize = 0;

    while !exploder.ended {
        if cpos >= data.len() {
            break;
        }
        let abuf = &data[cpos..];
        match exploder.explode_block(abuf) {
            Ok((consumed, block)) => {
                cpos += consumed;
                let end = out_pos + block.len();
                if end > out.len() {
                    return Err(MpqError::DecompressionFailed(
                        "PKWARE uncompressed output exceeds target buffer".to_string(),
                    ));
                }
                out[out_pos..end].copy_from_slice(block);
                out_pos = end;
                if consumed == 0 && block.is_empty() {
                    break;
                }
            }
            Err(e) => {
                return Err(MpqError::DecompressionFailed(format!(
                    "PKWARE explode block failed: {e:?}"
                )));
            }
        }
    }

    Ok(out_pos)
}
