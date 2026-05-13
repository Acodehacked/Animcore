use serde::{Deserialize, Serialize};

/// A single verb in a path, paired with points from the `points` buffer.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum PathVerb {
    MoveTo,     // 1 point
    LineTo,     // 1 point
    CubicTo,    // 3 points (cp1, cp2, end)
    QuadTo,     // 2 points (cp, end)
    Close,      // 0 points
}

/// Compact 2D path: flat `points` buffer + verb list, same layout as Skia/vello.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AnimPath {
    pub verbs: Vec<PathVerb>,
    pub points: Vec<[f32; 2]>,
}

impl AnimPath {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.verbs.push(PathVerb::MoveTo);
        self.points.push([x, y]);
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.verbs.push(PathVerb::LineTo);
        self.points.push([x, y]);
        self
    }

    pub fn cubic_to(
        &mut self,
        cx1: f32, cy1: f32,
        cx2: f32, cy2: f32,
        x: f32, y: f32,
    ) -> &mut Self {
        self.verbs.push(PathVerb::CubicTo);
        self.points.extend_from_slice(&[[cx1, cy1], [cx2, cy2], [x, y]]);
        self
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        self.verbs.push(PathVerb::QuadTo);
        self.points.extend_from_slice(&[[cx, cy], [x, y]]);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.verbs.push(PathVerb::Close);
        self
    }

    /// Convenience: closed rectangle.
    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        let mut p = Self::new();
        p.move_to(x, y)
            .line_to(x + w, y)
            .line_to(x + w, y + h)
            .line_to(x, y + h)
            .close();
        p
    }

    /// Approximate ellipse with four cubic beziers.
    pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Self {
        // magic constant for circle approximation with cubics
        const K: f32 = 0.5522847498;
        let kx = rx * K;
        let ky = ry * K;
        let mut p = Self::new();
        p.move_to(cx + rx, cy);
        p.cubic_to(cx + rx, cy - ky, cx + kx, cy - ry, cx, cy - ry);
        p.cubic_to(cx - kx, cy - ry, cx - rx, cy - ky, cx - rx, cy);
        p.cubic_to(cx - rx, cy + ky, cx - kx, cy + ry, cx, cy + ry);
        p.cubic_to(cx + kx, cy + ry, cx + rx, cy + ky, cx + rx, cy);
        p.close();
        p
    }

    /// Parse an SVG path `d` attribute string into an AnimPath.
    pub fn from_svg_d(d: &str) -> Result<Self, SvgPathError> {
        SvgPathParser::parse(d)
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }
}

#[derive(Debug)]
pub struct SvgPathError(pub String);

impl std::fmt::Display for SvgPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVG path parse error: {}", self.0)
    }
}

// ── SVG path `d` parser ──────────────────────────────────────────────────────

struct SvgPathParser<'a> {
    src: &'a [u8],
    pos: usize,
    path: AnimPath,
    current: [f32; 2],
    start: [f32; 2],
}

impl<'a> SvgPathParser<'a> {
    fn parse(d: &str) -> Result<AnimPath, SvgPathError> {
        let mut p = SvgPathParser {
            src: d.as_bytes(),
            pos: 0,
            path: AnimPath::new(),
            current: [0.0, 0.0],
            start: [0.0, 0.0],
        };
        p.run()?;
        Ok(p.path)
    }

    fn run(&mut self) -> Result<(), SvgPathError> {
        while self.pos < self.src.len() {
            self.skip_whitespace_and_commas();
            if self.pos >= self.src.len() {
                break;
            }
            let cmd = self.src[self.pos] as char;
            if !cmd.is_ascii_alphabetic() {
                return Err(SvgPathError(format!("unexpected char '{}' at {}", cmd, self.pos)));
            }
            self.pos += 1;
            self.dispatch(cmd)?;
        }
        Ok(())
    }

    fn dispatch(&mut self, cmd: char) -> Result<(), SvgPathError> {
        let relative = cmd.is_lowercase();
        match cmd.to_ascii_uppercase() {
            'M' => {
                let raw = self.read_pair()?;
                let (x, y) = self.abs_or_rel(raw, relative);
                self.path.move_to(x, y);
                self.current = [x, y];
                self.start = [x, y];
                while self.peek_number() {
                    let raw = self.read_pair()?;
                    let (x, y) = self.abs_or_rel(raw, relative);
                    self.path.line_to(x, y);
                    self.current = [x, y];
                }
            }
            'L' => {
                while self.peek_number() {
                    let raw = self.read_pair()?;
                    let (x, y) = self.abs_or_rel(raw, relative);
                    self.path.line_to(x, y);
                    self.current = [x, y];
                }
            }
            'H' => {
                while self.peek_number() {
                    let v = self.read_number()?;
                    let x = if relative { self.current[0] + v } else { v };
                    let cy = self.current[1];
                    self.path.line_to(x, cy);
                    self.current[0] = x;
                }
            }
            'V' => {
                while self.peek_number() {
                    let v = self.read_number()?;
                    let y = if relative { self.current[1] + v } else { v };
                    let cx = self.current[0];
                    self.path.line_to(cx, y);
                    self.current[1] = y;
                }
            }
            'C' => {
                while self.peek_number() {
                    let r1 = self.read_pair()?;
                    let (cx1, cy1) = self.abs_or_rel(r1, relative);
                    let r2 = self.read_pair()?;
                    let (cx2, cy2) = self.abs_or_rel(r2, relative);
                    let r3 = self.read_pair()?;
                    let (x, y) = self.abs_or_rel(r3, relative);
                    self.path.cubic_to(cx1, cy1, cx2, cy2, x, y);
                    self.current = [x, y];
                }
            }
            'Q' => {
                while self.peek_number() {
                    let r1 = self.read_pair()?;
                    let (cx, cy) = self.abs_or_rel(r1, relative);
                    let r2 = self.read_pair()?;
                    let (x, y) = self.abs_or_rel(r2, relative);
                    self.path.quad_to(cx, cy, x, y);
                    self.current = [x, y];
                }
            }
            'Z' => {
                self.path.close();
                self.current = self.start;
            }
            other => {
                return Err(SvgPathError(format!("unsupported command '{other}'")));
            }
        }
        Ok(())
    }

    fn abs_or_rel(&self, (x, y): (f32, f32), relative: bool) -> (f32, f32) {
        if relative {
            (self.current[0] + x, self.current[1] + y)
        } else {
            (x, y)
        }
    }

    fn read_pair(&mut self) -> Result<(f32, f32), SvgPathError> {
        let x = self.read_number()?;
        self.skip_whitespace_and_commas();
        let y = self.read_number()?;
        Ok((x, y))
    }

    fn read_number(&mut self) -> Result<f32, SvgPathError> {
        self.skip_whitespace_and_commas();
        let start = self.pos;
        if self.pos < self.src.len() && self.src[self.pos] == b'-' {
            self.pos += 1;
        }
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_digit() || self.src[self.pos] == b'.')
        {
            self.pos += 1;
        }
        // scientific notation
        if self.pos < self.src.len()
            && (self.src[self.pos] == b'e' || self.src[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.src.len()
                && (self.src[self.pos] == b'+' || self.src[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| SvgPathError("utf8 error".into()))?;
        s.parse::<f32>()
            .map_err(|_| SvgPathError(format!("bad number '{s}'")))
    }

    fn peek_number(&self) -> bool {
        let mut i = self.pos;
        while i < self.src.len()
            && (self.src[i] == b' '
                || self.src[i] == b'\t'
                || self.src[i] == b'\n'
                || self.src[i] == b','
                || self.src[i] == b'\r')
        {
            i += 1;
        }
        if i >= self.src.len() {
            return false;
        }
        let b = self.src[i];
        b.is_ascii_digit() || b == b'-' || b == b'.'
    }

    fn skip_whitespace_and_commas(&mut self) {
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' | b',' => self.pos += 1,
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_rect_path() {
        let p = AnimPath::from_svg_d("M 10 20 L 110 20 L 110 80 L 10 80 Z").unwrap();
        assert_eq!(p.verbs.len(), 5); // MoveTo + 3 LineTo + Close
    }

    #[test]
    fn parse_cubic() {
        let p = AnimPath::from_svg_d("M0,0 C10,0 90,100 100,100").unwrap();
        assert!(p.verbs.contains(&PathVerb::CubicTo));
    }

    #[test]
    fn relative_lineto() {
        let p = AnimPath::from_svg_d("M 50 50 l 100 0 l 0 50 z").unwrap();
        assert_eq!(p.verbs.len(), 4);
    }

    #[test]
    fn ellipse_approximate() {
        let p = AnimPath::ellipse(100.0, 100.0, 50.0, 30.0);
        assert!(!p.is_empty());
        assert_eq!(p.verbs.last(), Some(&PathVerb::Close));
    }
}
