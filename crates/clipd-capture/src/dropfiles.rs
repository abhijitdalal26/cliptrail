//! `CF_HDROP` payload (a `DROPFILES` header + wide, double-NUL-terminated
//! path list). Pure encoding; reading is done with `DragQueryFileW`.

/// Size of the `DROPFILES` header in bytes.
pub const HEADER_LEN: usize = 20;

/// Encode `paths` as a DROPFILES block (wide characters).
pub fn encode(paths: &[String]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(HEADER_LEN + paths.iter().map(|p| p.len() * 2 + 2).sum::<usize>() + 2);
    out.extend_from_slice(&(HEADER_LEN as u32).to_le_bytes()); // pFiles
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide
    for p in paths {
        for u in p.encode_utf16().filter(|&u| u != 0) {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes()); // list terminator
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal reference decoder for the tests.
    fn decode(data: &[u8]) -> Vec<String> {
        let off = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        assert_eq!(i32::from_le_bytes(data[16..20].try_into().unwrap()), 1);
        let units: Vec<u16> = data[off..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        units
            .split(|&u| u == 0)
            .take_while(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }

    #[test]
    fn encodes_header_and_double_null() {
        let paths = vec![r"C:\a b\c.txt".to_string(), r"D:\日本語\é.png".to_string()];
        let data = encode(&paths);
        assert_eq!(u32::from_le_bytes(data[0..4].try_into().unwrap()), 20);
        assert_eq!(&data[data.len() - 4..], &[0, 0, 0, 0]);
        assert_eq!(decode(&data), paths);
    }

    #[test]
    fn empty_list_is_just_terminator() {
        let data = encode(&[]);
        assert_eq!(data.len(), HEADER_LEN + 2);
        assert!(decode(&data).is_empty());
    }
}
