//! zlib 压缩（flate2 crate 直通——存档体积；read/write 对称）。

/// zlib 压缩（默认压缩级）
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    e.write_all(data).ok();
    e.finish().unwrap_or_default()
}

/// zlib 解压（损坏数据 = 空——调用方以空判损，不做异常穿透）
pub fn decompress(data: &[u8]) -> Vec<u8> {
    let mut d = flate2::read::ZlibDecoder::new(std::io::Cursor::new(data));
    let mut out = Vec::new();
    use std::io::Read;
    d.read_to_end(&mut out).ok();
    out
}
