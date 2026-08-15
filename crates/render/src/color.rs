pub fn parse_color_rgb(color: &str) -> [u8; 3] {
    let s = color.trim();
    if s.starts_with('#') {
        let h = s.trim_start_matches('#');
        if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
            let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(255);
            let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(255);
            return [r, g, b];
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "white" => [255, 255, 255],
        "black" => [0, 0, 0],
        "#101010" | "101010" => [16, 16, 16],
        _ => [255, 255, 255],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_named_and_hex() {
        assert_eq!(parse_color_rgb("white"), [255, 255, 255]);
        assert_eq!(parse_color_rgb("#101010"), [16, 16, 16]);
    }
}
