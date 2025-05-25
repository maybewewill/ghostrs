use std::fs::{File};
use std::io::{self, Read, Seek, Write};
use std::path::Path;

pub type ByteArray = Vec<u8>;

pub fn append_byte_array_size(b: &mut ByteArray, a: &[u8], size: usize) {
    b.extend_from_slice(&create_byte_array_size(a, size));
}

pub fn create_byte_array_size(a: &[u8], size: usize) -> ByteArray {
    if size < 1 || size > a.len() {
        return ByteArray::new();
    }
    a[..size].to_vec()
}

pub fn byte_array_to_uint16(data: &ByteArray, big_endian: bool, offset: usize) -> u16 {
    let slice = &data[offset..offset + 2];
    if big_endian {
        u16::from_be_bytes(slice.try_into().unwrap())
    } else {
        u16::from_le_bytes(slice.try_into().unwrap())
    }
}

pub fn byte_array_to_uint32(data: &ByteArray, big_endian: bool, offset: usize) -> u32 {
    let slice = &data[offset..offset + 4];
    if big_endian {
        u32::from_be_bytes(slice.try_into().unwrap())
    } else {
        u32::from_le_bytes(slice.try_into().unwrap())
    }
}


pub fn create_byte_array(data: &[u8]) -> ByteArray {
    data.to_vec()
}

pub fn create_byte_array_from_char(c: u8) -> ByteArray {
    vec![c]
}

pub fn create_byte_array_from_u16(i: u16, reverse: bool) -> ByteArray {
    let bytes = if reverse { i.to_be_bytes() } else { i.to_le_bytes() };
    bytes.to_vec()
}

pub fn create_byte_array_from_u32(i: u32, reverse: bool) -> ByteArray {
    let bytes = if reverse { i.to_be_bytes() } else { i.to_le_bytes() };
    bytes.to_vec()
}

pub fn byte_array_to_u16(b: &ByteArray, reverse: bool, start: usize) -> u16 {
    if b.len() < start + 2 {
        return 0;
    }
    let bytes = &b[start..start + 2];
    if reverse {
        u16::from_be_bytes([bytes[1], bytes[0]])
    } else {
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

pub fn byte_array_to_u32(b: &ByteArray, reverse: bool, start: usize) -> u32 {
    if b.len() < start + 4 {
        return 0;
    }
    let bytes = &b[start..start + 4];
    if reverse {
        u32::from_be_bytes([bytes[3], bytes[2], bytes[1], bytes[0]])
    } else {
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

pub fn byte_array_to_dec_string(b: &ByteArray) -> String {
    if b.is_empty() {
        return String::new();
    }
    let mut result = b[0].to_string();
    for &byte in b.iter().skip(1) {
        result.push_str(&format!(" {}", byte));
    }
    result
}

pub fn byte_array_to_hex_string(b: &ByteArray) -> String {
    if b.is_empty() {
        return String::new();
    }
    let mut result = format!("{:02x}", b[0]);
    for &byte in b.iter().skip(1) {
        result.push_str(&format!(" {:02x}", byte));
    }
    result
}

pub fn append_byte_array(b: &mut ByteArray, append: ByteArray) {
    b.extend(append);
}

pub fn append_byte_array_fast(b: &mut ByteArray, append: ByteArray) {
    b.extend(append);
}

pub fn append_byte_array_from_raw(b: &mut ByteArray, a: &[u8]) {
    b.extend(a);
}

pub fn append_byte_array_from_string(b: &mut ByteArray, append: &str, terminator: bool) {
    b.extend(append.as_bytes());
    if terminator {
        b.push(0);
    }
}

pub fn append_byte_array_fast_from_string(b: &mut ByteArray, append: &str, terminator: bool) {
    b.extend(append.as_bytes());
    if terminator {
        b.push(0);
    }
}

pub fn append_byte_array_from_u16(b: &mut ByteArray, i: u16, reverse: bool) {
    b.extend(create_byte_array_from_u16(i, reverse));
}

pub fn append_byte_array_from_u32(b: &mut ByteArray, i: u32, reverse: bool) {
    b.extend(create_byte_array_from_u32(i, reverse));
}

pub fn extract_cstring(b: &ByteArray, start: usize) -> ByteArray {
    if start >= b.len() {
        return Vec::new();
    }
    let end = b[start..].iter().position(|&x| x == 0).map_or(b.len(), |i| start + i);
    b[start..end].to_vec()
}

pub fn extract_hex(b: &ByteArray, start: usize, reverse: bool) -> u8 {
    if start + 1 >= b.len() {
        return 0;
    }
    let s = String::from_utf8_lossy(&b[start..start + 2]);
    let s = if reverse { s.chars().rev().collect::<String>() } else { s.to_string() };
    u8::from_str_radix(&s, 16).unwrap_or(0)
}

pub fn extract_numbers(s: &str, count: usize) -> ByteArray {
    let mut result = Vec::new();
    let mut iter = s.split_whitespace();
    for _ in 0..count {
        if let Some(num) = iter.next() {
            if let Ok(n) = num.parse::<u8>() {
                result.push(n);
            }
        }
    }
    result
}

pub fn extract_hex_numbers(s: &str) -> ByteArray {
    let mut result = Vec::new();
    for num in s.split_whitespace() {
        if let Ok(n) = u8::from_str_radix(num, 16) {
            result.push(n);
        }
    }
    result
}

pub fn to_string(i: u64) -> String {
    i.to_string()
}

pub fn to_string_i16(i: i16) -> String {
    i.to_string()
}

pub fn to_string_i32(i: i32) -> String {
    i.to_string()
}

pub fn to_string_f32(f: f32, digits: usize) -> String {
    format!("{:.1$}", f, digits)
}

pub fn to_string_f64(f: f64, digits: usize) -> String {
    format!("{:.1$}", f, digits)
}

pub fn to_hex_string(i: u32) -> String {
    format!("{:x}", i)
}

pub fn to_u16(s: &str) -> u16 {
    s.parse().unwrap_or(0)
}

pub fn to_u32(s: &str) -> u32 {
    s.parse().unwrap_or(0)
}

pub fn to_i16(s: &str) -> i16 {
    s.parse().unwrap_or(0)
}

pub fn to_i32(s: &str) -> i32 {
    s.parse().unwrap_or(0)
}

pub fn to_f64(s: &str) -> f64 {
    s.parse().unwrap_or(0.0)
}

pub fn ms_to_string(ms: u32) -> String {
    let mins = ms / 1000 / 60;
    let secs = (ms / 1000) % 60;
    format!("{:02}m{:02}s", mins, secs)
}

pub fn file_exists(file: &str) -> bool {
    Path::new(file).exists()
}

pub fn file_read(file: &str, start: u32, length: u32) -> io::Result<String> {
    let mut file = File::open(file)?;
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start as u64))?;
    }
    let mut buffer = vec![0; length as usize];
    let n = file.read(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer[..n]).to_string())
}

pub fn file_read_full(file: &str) -> io::Result<String> {
    let mut file = File::open(file)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

pub fn file_read_full_bytes(file: &str) -> io::Result<Vec<u8>> {
    let mut file = File::open(file)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}


pub fn file_read_size(file: &str) -> io::Result<usize> {
    let mut file = File::open(file)?;
    let mut buffer = Vec::new();
    let size = file.read_to_end(&mut buffer)?;
    Ok(size)
}

pub fn file_write(file: &str, data: &[u8]) -> io::Result<()> {
    let mut file = File::create(file)?;
    file.write_all(data)?;
    Ok(())
}

pub fn file_safe_name(file_name: &str) -> String {
    file_name.replace(['\\', '/', ':', '*', '?', '<', '>', '|'], "_")
}

pub fn add_path_separator(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let sep = if cfg!(windows) { '\\' } else { '/' };
    if path.ends_with(sep) {
        path.to_string()
    } else {
        format!("{}{}", path, sep)
    }
}

pub fn encode_stat_string(data: &ByteArray) -> ByteArray {
    let mut result = Vec::new();
    let mut mask = 1u8;
    for (i, &byte) in data.iter().enumerate() {
        if byte % 2 == 0 {
            result.push(byte + 1);
        } else {
            result.push(byte);
            mask |= 1 << ((i % 7) + 1);
        }
        if i % 7 == 6 || i == data.len() - 1 {
            result.insert(result.len() - 1 - (i % 7), mask);
            mask = 1;
        }
    }
    result
}

pub fn decode_stat_string(data: &ByteArray) -> ByteArray {
    let mut result = Vec::new();
    let mut mask = 0u8;
    for (i, &byte) in data.iter().enumerate() {
        if i % 8 == 0 {
            mask = byte;
        } else if (mask & (1 << (i % 8))) == 0 {
            result.push(byte - 1);
        } else {
            result.push(byte);
        }
    }
    result
}

pub fn is_lan_ip(ip: &ByteArray) -> bool {
    if ip.len() != 4 {
        return false;
    }
    (ip[0] == 127 && ip[1] == 0 && ip[2] == 0 && ip[3] == 1) ||
    (ip[0] == 10) ||
    (ip[0] == 172 && ip[1] >= 16 && ip[1] <= 31) ||
    (ip[0] == 192 && ip[1] == 168) ||
    (ip[0] == 169 && ip[1] == 254)
}

pub fn is_local_ip(ip: &ByteArray, local_ips: &[ByteArray]) -> bool {
    if ip.len() != 4 {
        return false;
    }
    local_ips.iter().any(|local| {
        local.len() == 4 && ip[0] == local[0] && ip[1] == local[1] && ip[2] == local[2] && ip[3] == local[3]
    })
}

pub fn replace(text: &mut String, key: &str, value: &str) {
    if value.contains(key) {
        return;
    }
    while let Some(pos) = text.find(key) {
        text.replace_range(pos..pos + key.len(), value);
    }
}

pub fn tokenize(s: &str, delim: char) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    for c in s.chars() {
        if c == delim {
            if !token.is_empty() {
                tokens.push(token);
                token = String::new();
            }
        } else {
            token.push(c);
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

pub fn factorial(x: u32) -> u32 {
    let mut result = 1;
    for i in 2..=x {
        result *= i;
    }
    result
}